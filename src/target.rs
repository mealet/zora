use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub triple: String,
    pub arch: String,
    pub os: String,
    pub env: String,
    pub family: String,
}

impl Target {
    pub fn resolve(triple: Option<&str>) -> Self {
        match triple {
            Some(triple) => Self::parse(triple),
            None => Self::host(),
        }
    }

    pub fn parse(triple: &str) -> Self {
        let mut parts = triple.split('-');
        let arch = parts.next().unwrap_or(triple).to_owned();
        let _vendor = parts.next().unwrap_or("unknown");
        let os = parts.next().unwrap_or("unknown").to_owned();
        let env = parts.next().unwrap_or_default().to_owned();
        let family = Self::derive_family(&os, &arch);

        Self {
            triple: triple.to_owned(),
            arch,
            os,
            env,
            family,
        }
    }

    pub fn host() -> Self {
        let arch = match std::env::consts::ARCH {
            "x86" => "i686",
            "arm64" => "aarch64",
            other => other,
        };

        let (vendor, os, env) = match std::env::consts::OS {
            "linux" => (
                "unknown",
                "linux",
                if Self::on_musl(arch) { "musl" } else { "gnu" },
            ),
            "macos" => ("apple", "darwin", ""),
            "windows" => ("pc", "windows", "msvc"),
            other => ("unknown", other, ""),
        };

        let triple = if env.is_empty() {
            format!("{arch}-{vendor}-{os}")
        } else {
            format!("{arch}-{vendor}-{os}-{env}")
        };
        let family = Self::derive_family(os, arch);

        Self {
            triple,
            arch: arch.to_owned(),
            os: os.to_owned(),
            env: env.to_owned(),
            family,
        }
    }

    pub fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    fn derive_family(os: &str, arch: &str) -> String {
        match os {
            "linux" | "macos" | "darwin" | "freebsd" | "openbsd" | "netbsd" | "dragonfly"
            | "android" => "unix".into(),
            "windows" => "windows".into(),
            _ if arch == "wasm32" || arch == "wasm64" => "wasm".into(),
            _ => String::new(),
        }
    }

    fn on_musl(arch: &str) -> bool {
        if std::env::consts::OS != "linux" {
            return false;
        }

        let loader = match arch {
            "x86_64" => "/lib/ld-musl-x86_64.so.1",
            "i686" => "/lib/ld-musl-i386.so.1",
            "aarch64" => "/lib/ld-musl-aarch64.so.1",
            _ => return false,
        };

        Path::new(loader).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_triple_fields() {
        let target = Target::parse("x86_64-pc-windows-msvc");

        assert_eq!(target.arch, "x86_64");
        assert_eq!(target.os, "windows");
        assert_eq!(target.env, "msvc");
        assert_eq!(target.family, "windows");
        assert!(target.is_windows());
    }

    #[test]
    fn parses_triple_without_env() {
        let target = Target::parse("x86_64-apple-darwin");

        assert_eq!(target.os, "darwin");
        assert_eq!(target.env, "");
        assert_eq!(target.family, "unix");
        assert!(!target.is_windows());
    }

    #[test]
    fn android_triple_is_unix() {
        let target = Target::parse("aarch64-linux-android");

        assert_eq!(target.arch, "aarch64");
        assert_eq!(target.os, "android");
        assert_eq!(target.family, "unix");
    }

    #[test]
    fn wasm_triple_is_wasm() {
        let target = Target::parse("wasm32-unknown-wasip1");

        assert_eq!(target.os, "wasip1");
        assert_eq!(target.family, "wasm");
    }
}
