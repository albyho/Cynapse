//! Shared user-facing strings for the Cynapse Client.
//!
//! `en` is the source language and fallback; `zh-CN` is a translation.
//! Rust code uses stable keys and never embeds display copy.

rust_i18n::i18n!("locales", fallback = "en");

pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

pub fn locale() -> String {
    rust_i18n::locale().to_string()
}

pub fn t(key: &str) -> String {
    rust_i18n::t!(key).into_owned()
}

/// Translate a key with `%{name}` placeholders.
pub fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    let template = rust_i18n::t!(key);
    if args.is_empty() {
        return template.into_owned();
    }

    let names = args.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let values = args
        .iter()
        .map(|(_, value)| (*value).to_string())
        .collect::<Vec<_>>();
    rust_i18n::replace_patterns(&template, &names, &values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    static LOCALE_LOCK: Mutex<()> = Mutex::new(());

    fn locale_guard() -> std::sync::MutexGuard<'static, ()> {
        LOCALE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn merge_catalogs(sources: &[&str]) -> BTreeMap<String, String> {
        let mut catalog = BTreeMap::new();
        for source in sources {
            let part: BTreeMap<String, String> =
                serde_saphyr::from_str(source).expect("catalog part should parse");
            catalog.extend(part);
        }
        catalog
    }

    fn catalogs() -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let en = merge_catalogs(&[
            include_str!("../locales/common.en.yml"),
            include_str!("../locales/vscode.en.yml"),
            include_str!("../locales/jupyterlab.en.yml"),
        ]);
        let zh = merge_catalogs(&[
            include_str!("../locales/common.zh-CN.yml"),
            include_str!("../locales/vscode.zh-CN.yml"),
            include_str!("../locales/jupyterlab.zh-CN.yml"),
        ]);
        (en, zh)
    }

    fn without_version(catalog: BTreeMap<String, String>) -> BTreeMap<String, String> {
        catalog
            .into_iter()
            .filter(|(key, _)| key != "_version")
            .collect()
    }

    fn placeholders(message: &str) -> BTreeMap<String, ()> {
        let mut names = BTreeMap::new();
        let mut rest = message;
        while let Some(start) = rest.find("%{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find('}') else {
                break;
            };
            names.insert(after[..end].to_string(), ());
            rest = &after[end + 1..];
        }
        names
    }

    #[test]
    fn translates_the_active_locale() {
        let _guard = locale_guard();
        set_locale("zh-CN");
        assert_eq!(t("next.step"), "下一步");

        set_locale("en");
        assert_eq!(t("next.step"), "Next step");
    }

    #[test]
    fn falls_back_to_english() {
        let _guard = locale_guard();
        set_locale("fr");
        assert_eq!(t("next.step"), "Next step");
        set_locale("en");
    }

    #[test]
    fn substitutes_named_placeholders() {
        let _guard = locale_guard();
        set_locale("en");
        assert_eq!(
            t_with("error.challenge_check_failed", &[("error", "timed out")]),
            "Challenge check failed: timed out"
        );

        set_locale("zh-CN");
        assert_eq!(
            t_with("error.challenge_check_failed", &[("error", "超时")]),
            "挑战检查失败：超时"
        );
        set_locale("en");
    }

    #[test]
    fn action_keys_resolve() {
        let _guard = locale_guard();
        set_locale("en");
        assert_eq!(t("action.insert_text"), "Inserting text: %{path}");
        assert_eq!(t("action.present"), "Presenting narration");
        assert_eq!(t("action.open_preview"), "Opening preview: %{path}");
        assert_eq!(t("action.close_document"), "Closing document: %{path}");
        assert_eq!(t("action.restart_kernel"), "Restarting kernel: %{path}");
        assert_eq!(
            t_with("action.insert_text", &[("path", "src/main.rs")]),
            "Inserting text: src/main.rs"
        );
        set_locale("en");
    }

    #[test]
    fn client_error_messages_exist_in_both_locales() {
        let _guard = locale_guard();

        set_locale("en");
        assert_eq!(
            t("error.adapter_disconnected"),
            "The required target software is not connected. Connect it and start learning again."
        );
        assert_eq!(
            t("error.challenge_check_timeout"),
            "Challenge check timed out. The Adapter command was cancelled. Confirm the target software is connected and try again."
        );
        assert_eq!(
            t("error.workspace_reset_failed"),
            "Workspace reset failed: %{error}"
        );

        set_locale("zh-CN");
        assert_eq!(
            t("error.adapter_disconnected"),
            "所需目标软件未连接。连接后重新开始学习。"
        );
        assert_eq!(
            t("error.challenge_check_timeout"),
            "挑战检查超时，已取消 Adapter 命令。请确认目标软件已连接后重试。"
        );
        set_locale("en");
    }

    #[test]
    fn catalogs_have_the_same_keys() {
        let (en, zh) = catalogs();
        let en = without_version(en);
        let zh = without_version(zh);

        let en_keys = en.keys().collect::<Vec<_>>();
        let zh_keys = zh.keys().collect::<Vec<_>>();
        assert_eq!(en_keys, zh_keys, "en.yml and zh-CN.yml keys must match");
    }

    #[test]
    fn each_catalog_part_has_the_same_keys() {
        for (en_source, zh_source) in [
            (
                include_str!("../locales/common.en.yml"),
                include_str!("../locales/common.zh-CN.yml"),
            ),
            (
                include_str!("../locales/vscode.en.yml"),
                include_str!("../locales/vscode.zh-CN.yml"),
            ),
            (
                include_str!("../locales/jupyterlab.en.yml"),
                include_str!("../locales/jupyterlab.zh-CN.yml"),
            ),
        ] {
            let en = without_version(merge_catalogs(&[en_source]));
            let zh = without_version(merge_catalogs(&[zh_source]));
            assert_eq!(
                en.keys().collect::<Vec<_>>(),
                zh.keys().collect::<Vec<_>>(),
                "catalog part keys must match"
            );
        }
    }

    #[test]
    fn catalogs_have_the_same_placeholders() {
        let (en, zh) = catalogs();
        for (key, value) in without_version(en) {
            let translated = zh.get(&key).expect("key parity test should catch this");
            assert_eq!(
                placeholders(&value),
                placeholders(translated),
                "placeholders differ for {key}"
            );
        }
    }
}
