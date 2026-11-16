//! 当前版本和发行说明地址；构建来源由 build.rs 提供。

const REPO: &str = env!("CARGO_PKG_REPOSITORY");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// "release"（发行工作流或匹配版本 tag 的 CI）、"test"（其他 CI）或 "local"。
const KIND: &str = env!("PONI_BUILD_KIND");
/// The commit a CI build was made from, 7 characters; empty for a local build.
const COMMIT: &str = env!("PONI_BUILD_COMMIT");
/// 当前发行说明或测试构建的提交地址。
pub fn notes_url() -> String {
    url(VERSION, KIND, COMMIT)
}

fn url(version: &str, kind: &str, commit: &str) -> String {
    match kind {
        "release" => format!("{REPO}/releases/tag/v{version}"),
        "test" if !commit.is_empty() => format!("{REPO}/commit/{commit}"),
        _ => format!("{REPO}/releases"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_opens_its_notes_and_a_test_build_its_commit() {
        assert_eq!(
            url("0.2.3", "release", "c7d77c8"),
            "https://github.com/wucheng-work/island/releases/tag/v0.2.3"
        );
        assert_eq!(
            url("0.2.3", "test", "c7d77c8"),
            "https://github.com/wucheng-work/island/commit/c7d77c8"
        );
        assert_eq!(
            url("0.2.3", "local", ""),
            "https://github.com/wucheng-work/island/releases"
        );
    }
}
