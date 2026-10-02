//! 后台作业的运行/取消状态。刮削、Anki 导出、词典导入、补齐歌词共用这一份。
//!
//! **为什么要合成一份**：原来四处各写了一个一模一样的 `{ running, cancel }`，
//! 于是同一个 bug 写了四遍，修也只修得动一处：
//!
//! 1. **`try_start` 会抹掉别人的取消。** 三处写的是「先 `cancel.store(false)`
//!    再 `running.swap(true)`」——已经有作业在跑时抢不到运行权，却**已经把
//!    人家的取消标志清掉了**。用户点「取消」，紧接着（或并发地）点了一下
//!    「开始」，取消就没了。`dict.rs` 单独修过这一处并留了测试，另外三处没跟上。
//!
//! 2. **线程 panic 会把界面永久卡死。** 四处都是
//!    `thread::spawn(|| { let r = (||{...})(); job.finish(); emit(终态) })`：
//!    闭包里 panic 会直接展开出线程，`finish()` 和终态事件**都跳过**，
//!    `running` 从此是 `true`，按钮永久禁用——而且 `*_is_running` 重新挂载时
//!    问到的还是 `true`，不重启应用恢复不了。
//!
//! 这里把运行权做成 [`Guard`]：拿得到才跑，**离开作用域（含 panic 展开）
//! 一定把 `running` 放掉**，并且 panic 时还会补发一条终态事件，
//! 让界面至少知道「这事儿黄了」而不是永远转圈。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 一个后台作业的状态。各处用 `pub type XxxJob = Job;` 起自己的名字。
#[derive(Default)]
pub struct Job {
    running: AtomicBool,
    /// `Arc` 是因为词典导入要把这个标志整个传进 `jp_dict::ImportOptions`
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// 作业有没有被要求取消。长循环每一轮问一次。
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 取消标志本身，传给需要它的下层（词典导入）。
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// 抢占运行权。已经在跑就返回 `None`。
    ///
    /// **顺序很重要**：先占住 `running`，占到了才清取消标志。
    /// 反过来写的话，抢不到运行权的那一次会把正在跑的那个作业的取消抹掉。
    pub fn start(self: &Arc<Self>) -> Option<Guard> {
        if self.running.swap(true, Ordering::SeqCst) {
            return None;
        }
        self.cancel.store(false, Ordering::SeqCst);
        Some(Guard {
            job: Arc::clone(self),
            on_panic: None,
        })
    }

    /// 同 [`Job::start`]，但额外给一个「线程 panic 了」时补发终态的回调。
    ///
    /// 正常结束时**不会**调它——正常路径上各作业要发的终态带着统计数字，
    /// 由各自的代码发。这里只管「崩了」这一种，发一条最朴素的收尾。
    pub fn start_with(
        self: &Arc<Self>,
        on_panic: impl FnOnce() + Send + 'static,
    ) -> Option<Guard> {
        let mut guard = self.start()?;
        guard.on_panic = Some(Box::new(on_panic));
        Some(guard)
    }
}

/// 运行权。活着表示作业在跑，drop 掉就放开——**panic 展开时也会 drop**。
pub struct Guard {
    job: Arc<Job>,
    on_panic: Option<Box<dyn FnOnce() + Send>>,
}

impl Guard {
    /// 作业有没有被要求取消。
    pub fn cancelled(&self) -> bool {
        self.job.cancelled()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.job.running.store(false, Ordering::SeqCst);
        if std::thread::panicking() {
            crate::log::error("后台作业线程 panic 了，运行标志已经放开");
            if let Some(on_panic) = self.on_panic.take() {
                on_panic();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_job_runs_at_a_time_and_the_slot_frees_up_afterwards() {
        let job = Arc::new(Job::default());
        let guard = job.start().expect("第一个该抢到");
        assert!(job.is_running());
        assert!(job.start().is_none(), "第二个作业抢到了运行权");
        drop(guard);
        assert!(!job.is_running());
        assert!(job.start().is_some(), "放开之后该能再起一个");
    }

    /// 这就是原来三处共同的那个 bug。
    #[test]
    fn a_second_start_cannot_wipe_out_the_running_jobs_cancel() {
        let job = Arc::new(Job::default());
        let _guard = job.start().unwrap();
        job.request_cancel();
        assert!(job.start().is_none());
        assert!(job.cancelled(), "第二次启动把正在跑的那个的取消抹掉了");
    }

    /// 新作业不该继承上一个的取消标志。
    #[test]
    fn a_new_job_starts_without_the_previous_cancel() {
        let job = Arc::new(Job::default());
        let guard = job.start().unwrap();
        job.request_cancel();
        drop(guard);
        let _guard = job.start().unwrap();
        assert!(!job.cancelled());
    }

    /// panic 展开时也要把运行权放掉，并且补发一次终态——
    /// 否则界面上的按钮永久禁用，重启才好得了。
    #[test]
    fn a_panicking_job_releases_the_slot_and_reports_a_finish() {
        let job = Arc::new(Job::default());
        let told = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&told);

        let for_thread = Arc::clone(&job);
        let handle = std::thread::spawn(move || {
            let _guard = for_thread
                .start_with(move || flag.store(true, Ordering::SeqCst))
                .unwrap();
            panic!("后台炸了");
        });
        assert!(handle.join().is_err(), "这个线程本来就该 panic");

        assert!(!job.is_running(), "panic 之后运行标志没放开，界面会永久卡住");
        assert!(told.load(Ordering::SeqCst), "panic 之后没补发终态");
        assert!(job.start().is_some(), "崩过之后还能再起一个");
    }

    /// 正常结束时不该调 `on_panic`——终态由各作业自己带着数字发。
    #[test]
    fn a_normal_finish_does_not_fire_the_panic_callback() {
        let job = Arc::new(Job::default());
        let told = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&told);
        {
            let _guard = job.start_with(move || flag.store(true, Ordering::SeqCst)).unwrap();
        }
        assert!(!told.load(Ordering::SeqCst));
        assert!(!job.is_running());
    }
}
