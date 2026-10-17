//! 后台作业：导入、下载分词词典。
//!
//! ## 为什么是轮询
//!
//! C ABI 只有「调一次、拿一个 JSON」这一种形状（见 `lib.rs`）。几分钟的导入如果也这样调，
//! 会话那把锁就被它占满几分钟，期间检索、播放定位全得排队。所以长作业是：
//!
//! 1. `runImport` / `tokenizerDownload` 启动一条线程，**立刻返回**；
//! 2. 线程**自己开一条 SQLite 连接**（库是 WAL，读写不互相挡），不碰会话那条；
//! 3. 界面隔几百毫秒调一次 `importProgress` / `tokenizerProgress` 拿 [`JobView`]，
//!    要停就调 `cancelImport`。
//!
//! 和 Tauri 客户端的区别只在「怎么把进度送出去」：那边发事件，这边等人来问。
//! 导入本身是同一个 `jp_import::execute_cancellable`，一首歌一个事务，停在哪儿哪儿之前的就是真导进去了。
//!
//! 回调（C 函数指针从 Rust 线程回调进 Swift）也能做，但要处理「回调时 Swift 那边的对象已经没了」、
//! 回调跑在哪条线程上、主线程派发……轮询没有这些问题，代价是进度最多晚几百毫秒。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::Value;

/// 界面轮询拿到的那一份。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    /// 在跑
    pub running: bool,
    /// 跑完了（成功、失败、取消都算）。启动过才会是 true
    pub finished: bool,
    /// 被取消了。取消也是正常收尾，`result` 里是取消前真做完的那些
    pub cancelled: bool,
    /// 做到第几个 / 一共几个。导入是首数，下载是字节
    pub done: u64,
    pub total: u64,
    /// 当前在做什么，给界面一句话（导入：刚导完的歌名；下载：`downloading` / `extracting` / `tokenizing`）
    pub stage: String,
    /// 跑完的结果（各作业自己的形状）。没跑完是 null
    pub result: Value,
    /// 失败的原因。成功、取消时是 null
    pub error: Option<String>,
}

/// 一个后台作业的状态。一种作业一个，同一时间只跑一份。
#[derive(Default)]
pub struct Job {
    view: Mutex<JobView>,
    /// Arc：词典导入（`jp_dict::import`）要拿着这个标志自己去看
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn view(&self) -> JobView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 取消标志本身，交给自己会看标志的库函数
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// 改进度。作业线程里调
    pub fn report(&self, done: u64, total: u64, stage: impl Into<String>) {
        let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
        view.done = done;
        view.total = total;
        view.stage = stage.into();
    }

    /// 起一条线程跑 `work`。已经在跑就报错，**不动**正在跑的那一份（包括它的取消标志）。
    ///
    /// `work` 返回 `Ok(结果)` 或错误；panic 也接住，变成一条错误——
    /// 不然 `running` 永远是 true，界面上的按钮就永久灰了（Tauri 那边踩过，见 `jp-app/src/job.rs`）。
    pub fn spawn(
        self: &Arc<Self>,
        what: &str,
        work: impl FnOnce(&Job) -> Result<(Value, bool)> + Send + 'static,
    ) -> Result<()> {
        {
            let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
            if view.running {
                return Err(anyhow!("已经有一个{what}在跑"));
            }
            // 先占住运行权，再清取消标志：顺序反了会抹掉正在跑的那个的取消
            *view = JobView { running: true, ..Default::default() };
            self.cancel.store(false, Ordering::SeqCst);
        }
        let job = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("jp-ffi {what}"))
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| work(&job)));
                let mut view = job.view.lock().unwrap_or_else(|e| e.into_inner());
                view.running = false;
                view.finished = true;
                match outcome {
                    Ok(Ok((result, cancelled))) => {
                        view.result = result;
                        view.cancelled = cancelled;
                    }
                    Ok(Err(err)) => view.error = Some(format!("{err:#}")),
                    Err(_) => view.error = Some("内部错误：后台线程崩了".into()),
                }
            })
            .map_err(|err| {
                let mut view = self.view.lock().unwrap_or_else(|e| e.into_inner());
                view.running = false;
                anyhow!("开不了后台线程：{err}")
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(job: &Job) -> JobView {
        for _ in 0..500 {
            let view = job.view();
            if view.finished {
                return view;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("作业没跑完");
    }

    #[test]
    fn a_second_start_is_refused_and_does_not_clear_the_first_ones_cancel() {
        let job = Arc::new(Job::default());
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        job.spawn("测试", move |job| {
            rx.recv().ok();
            Ok((Value::Null, job.cancelled()))
        })
        .unwrap();
        job.request_cancel();
        let err = job.spawn("测试", |_| Ok((Value::Null, false))).unwrap_err();
        assert!(err.to_string().contains("已经有一个测试在跑"));
        tx.send(()).unwrap();
        let view = wait(&job);
        assert!(view.cancelled && !view.running && view.error.is_none());
    }

    #[test]
    fn a_panicking_job_finishes_with_an_error_and_can_be_started_again() {
        let job = Arc::new(Job::default());
        job.spawn("测试", |_| panic!("炸了")).unwrap();
        let view = wait(&job);
        assert!(!view.running && view.error.as_deref().unwrap().contains("内部错误"));
        job.spawn("测试", |_| Ok((serde_json::json!(1), false))).unwrap();
        assert_eq!(wait(&job).result, serde_json::json!(1));
    }
}
