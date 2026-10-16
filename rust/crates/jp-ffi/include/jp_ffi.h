/*
 * JPOP Corpus core — C ABI for the native macOS client.
 *
 * Hand-written; mirrors the five functions in rust/crates/jp-ffi/src/lib.rs.
 * Every string is NUL-terminated UTF-8. Strings returned by this library must be
 * released with jp_string_free().
 *
 * jp_call() returns a JSON envelope: {"ok": <value>} or {"error": "<message>"}.
 * Method names and argument shapes match the Tauri commands (camelCase JSON),
 * e.g. jp_call(s, "kwic", "{\"query\":{\"keywords\":[\"夜\"],\"field\":\"lemma\"}}").
 */
#ifndef JP_FFI_H
#define JP_FFI_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct JpSession JpSession;

/* Opens (and migrates, like the Tauri client) the corpus at db_path.
 * Returns NULL on failure and stores a message in *out_error (may be NULL). */
JpSession *jp_open(const char *db_path, char **out_error);

/* Calls a core method. args_json may be NULL (same as "{}"). Never returns NULL. */
char *jp_call(const JpSession *session, const char *method, const char *args_json);

/* Closes a session. NULL is a no-op. */
void jp_close(JpSession *session);

/* Frees a string returned by this library. NULL is a no-op. */
void jp_string_free(char *s);

/* Writes the small synthetic corpus (four songs) into dir and returns the path of
 * its corpus.db, or NULL. For tests and first-run demos. */
char *jp_write_fixture(const char *dir);

#ifdef __cplusplus
}
#endif

#endif /* JP_FFI_H */
