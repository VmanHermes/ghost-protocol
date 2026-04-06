use std::path::PathBuf;

pub fn default_home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
}

pub fn expand_workdir(input: &str) -> String {
    let trimmed = input.trim();

    if trimmed.is_empty() || trimmed == "~" {
        return default_home_dir();
    }

    if let Some(rest) = trimmed.strip_prefix("~/") {
        return PathBuf::from(default_home_dir())
            .join(rest)
            .to_string_lossy()
            .into_owned();
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde_to_home() {
        let expected = default_home_dir();
        assert_eq!(expand_workdir("~"), expected);
        assert_eq!(expand_workdir(""), expected);
    }

    #[test]
    fn expands_tilde_subpaths() {
        let expected = format!("{}/projects/demo", default_home_dir());
        assert_eq!(expand_workdir("~/projects/demo"), expected);
    }

    #[test]
    fn leaves_other_paths_unchanged() {
        assert_eq!(expand_workdir("/tmp/demo"), "/tmp/demo");
        assert_eq!(expand_workdir("./demo"), "./demo");
    }
}
