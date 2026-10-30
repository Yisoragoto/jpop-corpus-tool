//! 资料库方法。慢调用/作业单开连接，不占会话锁。
use super::*;
use jp_corpus::members;

pub(super) fn dispatch(s: &Session, method: &str, args: &Value) -> Result<Option<Value>> {
    let v = match method {
        "libraryProgress" => json!(s.library_job.view()),
        "backfillDurations" | "fillAlbumArtwork" | "migrateRun" => {
            let db = s.db_path.clone();
            let root = s.library_root();
            let method = method.to_string();
            let source: Option<String> = arg_or(args, "path", None)?;
            let options = arg_or(
                args,
                "options",
                jp_import::migrate::MigrateOptions::default(),
            )?;
            let slot = Arc::clone(&s.analyzer);
            let resolve = s.media_resolver()?;
            s.library_job.spawn("资料库维护", move |job| {
                let mut corpus = open_worker(&db)?;
                let result = match method.as_str() {
                    "backfillDurations" => {
                        job.report(0, 0, "正在读取音频时长…");
                        json!(jp_library::maintenance::backfill_durations(&mut corpus)?)
                    }
                    "fillAlbumArtwork" => {
                        job.report(0, 0, "正在补专辑封面…");
                        // 专辑图找不到文件也算缺（从别的电脑拷来的库），换成同专辑里这台电脑上找得到的歌曲封面
                        json!(jp_scrape::fill_album_artwork_with(corpus.connection(), &resolve)?)
                    }
                    _ => {
                        let source = PathBuf::from(source.ok_or_else(|| anyhow!("没给源目录"))?);
                        ensure!(
                            std::fs::canonicalize(&source)? != std::fs::canonicalize(&root)?,
                            "源目录就是当前库，不用迁移"
                        );
                        let mut outcome = jp_library::migrate::run(
                            corpus.connection_mut(),
                            &source,
                            &root,
                            options,
                            |step| job.report(0, 0, step),
                        )?;
                        slot.reload(&root);
                        job.report(0, 0, "正在给没分词的歌补分词…");
                        match tokenize_backlog(&db, slot.get(&root)) {
                            Ok(report) => outcome.tokenized = report,
                            Err(err) => outcome.warnings.push(format!(
                                "补分词失败（已回滚，可在「补齐缺失分词」重试）：{err:#}"
                            )),
                        }
                        json!(outcome)
                    }
                };
                job.report(1, 1, "完成");
                Ok((result, false))
            })?;
            Value::Null
        }
        "lemmaForms" => {
            let lemmas: Vec<String> = arg(args, "lemmas")?;
            json!(
                s.analyzer()
                    .map(|a| a.lemma_forms(&lemmas))
                    .unwrap_or_default()
            )
        }
        "libraryNewAudio" => json!(jp_library::discovery::new_audio(
            &s.network_connection()?,
            &s.library_root(),
            &arg_or(args, "ignoredFolders", Vec::new())?,
            &arg_or(args, "ignoredFiles", Vec::new())?,
            s.analyzer().is_some()
        )?),
        "libraryEditSong"
        | "libraryDeleteSong"
        | "libraryMissingAudio"
        | "librarySuggestRelinks"
        | "libraryRelinkAudio" => {
            let mut corpus = s.network_connection()?;
            let mut library = jp_library::admin::Library {
                corpus: &mut corpus,
                db_path: &s.db_path,
                covers_dir: &s.covers_dir(),
                pitch_cache: &s.library_root().join("cache/pitch"),
            };
            match method {
                "libraryEditSong" => json!(jp_library::admin::edit_song(
                    &mut library,
                    &arg::<String>(args, "songId")?,
                    &arg(args, "edit")?
                )?),
                "libraryDeleteSong" => json!(jp_library::admin::delete_song(
                    &mut library,
                    &arg::<String>(args, "songId")?
                )?),
                "libraryMissingAudio" => json!(jp_library::admin::missing_audio(&library)?),
                "librarySuggestRelinks" => json!(jp_library::admin::suggest_relinks(
                    &library,
                    Path::new(&arg::<String>(args, "folder")?)
                )?),
                _ => json!(jp_library::admin::relink_audio(
                    &mut library,
                    &arg::<Vec<_>>(args, "links")?
                )?),
            }
        }
        "artistImageSet"
        | "artistImageReset"
        | "artistRoster"
        | "peopleByRole"
        | "worksByPerson"
        | "mostPlayed"
        | "recentlyPlayed"
        | "memberships"
        | "bandMembers"
        | "bandsOf"
        | "membershipSuggestions"
        | "membershipPut"
        | "membershipConfirm"
        | "membershipReject"
        | "tokenCorrection"
        | "saveTokenCorrection"
        | "revertTokenCorrection"
        | "migratePlan" => {
            let c = s.network_connection()?;
            let conn = c.connection();
            match method {
                "artistImageSet" => json!(jp_scrape::set_artist_image(
                    conn,
                    &s.artists_dir(),
                    &arg::<String>(args, "name")?,
                    Path::new(&arg::<String>(args, "path")?)
                )?),
                "artistImageReset" => json!(jp_scrape::reset_artist_image(
                    conn,
                    &s.artists_dir(),
                    &arg::<String>(args, "name")?
                )?),
                "artistRoster" => json!(jp_library::people::artist_roster(conn)?),
                "peopleByRole" => json!(
                    c.people_by_role(&arg::<String>(args, "role")?, arg_or(args, "limit", 200)?)?
                ),
                "worksByPerson" => json!(c.works_by_person(arg(args, "personId")?)?),
                "mostPlayed" => json!(c.most_played(
                    arg_or(args, "minListenedSec", 30.0)?,
                    arg_or(args, "limit", 20)?
                )?),
                "recentlyPlayed" => json!(c.recently_played(arg_or(args, "limit", 20)?)?),
                "memberships" => json!(members::all(conn)?),
                "bandMembers" => json!(members::members_of(conn, arg(args, "personId")?)?),
                "bandsOf" => json!(members::bands_of(conn, arg(args, "personId")?)?),
                "membershipSuggestions" => json!(members::suggestions(conn, 3, 0.8)?),
                "membershipPut" => json!(jp_library::people::membership_put(
                    conn,
                    arg(args, "bandId")?,
                    arg_or(args, "memberId", None)?,
                    arg_or(args, "memberName", None)?,
                    &arg::<String>(args, "part")?,
                    &arg::<String>(args, "joined")?,
                    &arg::<String>(args, "leftAt")?
                )?),
                "membershipConfirm" => json!(members::put(
                    conn,
                    arg(args, "bandId")?,
                    arg(args, "memberId")?,
                    "",
                    "",
                    "",
                    "confirmed"
                )?),
                "membershipReject" => {
                    members::reject(conn, arg(args, "bandId")?, arg(args, "memberId")?)?;
                    Value::Null
                }
                "tokenCorrection" => json!(c.token_correction(arg(args, "utteranceId")?)?),
                "saveTokenCorrection" => json!(c.save_token_correction(
                    arg(args, "utteranceId")?,
                    &arg::<Vec<_>>(args, "tokens")?
                )?),
                "revertTokenCorrection" => {
                    json!(c.revert_token_correction(arg(args, "utteranceId")?)?)
                }
                _ => {
                    let source = PathBuf::from(arg::<String>(args, "path")?);
                    ensure!(
                        std::fs::canonicalize(&source)? != std::fs::canonicalize(s.library_root())?,
                        "源目录就是当前库，不用迁移"
                    );
                    json!(jp_library::migrate::plan(conn, &source)?)
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(v))
}
