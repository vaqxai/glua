use zed_extension_api::{
    self as zed, settings::LspSettings, Architecture, Command, LanguageServerId, Os, Result,
    Worktree,
};

const REPO: &str = "Pollux12/gmod-glua-ls";
const BINARY_NAME: &str = "glua_ls";
const ANNOTATIONS_REPO: &str = "Pollux12/gmod-luals-addon";
const ANNOTATIONS_BRANCH: &str = "gluals-annotations";
/// The top-level folder inside the GitHub-generated zip archive.
const ANNOTATIONS_ZIP_INNER_FOLDER: &str = "gmod-luals-addon-gluals-annotations";
const ANNOTATIONS_DIR: &str = "gmod-annotations";

struct GluaExtension {
    cached_binary_path: Option<String>,
    cached_annotations_path: Option<String>,
}

/// Platform-specific asset metadata derived from the current OS and architecture.
struct AssetInfo {
    /// Full asset filename as it appears in the GitHub release (e.g. `glua_ls-linux-x64.tar.gz`).
    asset_name: String,
    /// How Zed should unpack the downloaded file.
    file_type: zed::DownloadedFileType,
    /// Name of the binary inside the archive.
    binary_name: String,
}

impl GluaExtension {
    /// Map Zed's (Os, Architecture) to the actual release asset published by
    /// the gmod-glua-ls GitHub Actions CI.
    ///
    /// Upstream CI produces these `glua_ls` assets per release:
    ///   glua_ls-linux-x64.tar.gz             (contains bare `glua_ls`)
    ///   glua_ls-linux-x64-glibc.2.17.tar.gz  (contains bare `glua_ls`)
    ///   glua_ls-win32-x64.zip                (contains bare `glua_ls.exe`)
    ///
    /// No macOS or aarch64 builds are published at this time.
    fn asset_info(os: Os, arch: Architecture) -> Result<AssetInfo> {
        match (os, arch) {
            (Os::Linux, Architecture::X8664) => Ok(AssetInfo {
                asset_name: "glua_ls-linux-x64.tar.gz".into(),
                file_type: zed::DownloadedFileType::GzipTar,
                binary_name: BINARY_NAME.into(),
            }),
            (Os::Windows, Architecture::X8664) => Ok(AssetInfo {
                asset_name: "glua_ls-win32-x64.zip".into(),
                file_type: zed::DownloadedFileType::Zip,
                binary_name: format!("{BINARY_NAME}.exe"),
            }),
            _ => Err(format!(
                "Unsupported platform: {os:?} / {arch:?}. \
                 gmod-glua-ls only publishes Linux x64 and Windows x64 binaries."
            )),
        }
    }

    fn language_server_binary(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<String> {
        // 1. Check if user has glua_ls on their PATH already.
        if let Some(path) = worktree.which(BINARY_NAME) {
            return Ok(path);
        }

        // 2. Return cached path if the binary is already downloaded.
        if let Some(path) = &self.cached_binary_path {
            if std::path::Path::new(path).exists() {
                return Ok(path.clone());
            }
        }

        // 3. Fetch the latest release from GitHub and download the binary.
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (os, arch) = zed::current_platform();
        let info = Self::asset_info(os, arch)?;

        let asset = release
            .assets
            .iter()
            .find(|a| a.name == info.asset_name)
            .ok_or_else(|| {
                format!(
                    "No asset '{}' found in release {}",
                    info.asset_name, release.version
                )
            })?;

        let install_dir = format!("{BINARY_NAME}-{}", release.version);
        let binary_path = format!("{install_dir}/{}", info.binary_name);

        // Skip download if we already have this version.
        if !std::path::Path::new(&binary_path).exists() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &install_dir,
                info.file_type,
            )
            .map_err(|e| format!("Failed to download {}: {e}", info.asset_name))?;

            // Make the binary executable on Unix.
            zed::make_file_executable(&binary_path)?;
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }

    /// Download GMod wiki annotations from the `gluals-annotations` branch of
    /// `Pollux12/gmod-luals-addon`.  The VSCode extension does this
    /// automatically; we replicate the behaviour here so that Zed users get
    /// globals like `CurTime`, `ParticleEmitter`, etc. out of the box.
    ///
    /// The zip archive is structured as:
    ///   gmod-luals-addon-gluals-annotations/
    ///     __metadata.json
    ///     ...lua annotation files...
    ///
    /// `zed::download_file` with `DownloadedFileType::Zip` extracts the
    /// contents into the target directory, so we end up with:
    ///   gmod-annotations/gmod-luals-addon-gluals-annotations/...
    ///
    /// IMPORTANT: We must return an **absolute host path** because the LSP
    /// runs outside the WASI sandbox with its working directory set to the
    /// user's project. A relative path or a sandbox-canonicalized path will
    /// silently fail to load.  We resolve the absolute path by reading
    /// `HOME`/`APPDATA` from the worktree's shell env and reconstructing the
    /// well-known Zed extension work-dir layout.
    fn ensure_annotations(&mut self, worktree: &Worktree) -> Result<String> {
        // Return cached path if still valid.
        if let Some(path) = &self.cached_annotations_path {
            // We only ever cache absolute paths now.  Recheck existence in
            // case the user nuked the cache directory between sessions.
            return Ok(path.clone());
        }

        let inner_path = format!("{ANNOTATIONS_DIR}/{ANNOTATIONS_ZIP_INNER_FOLDER}");

        // Download if needed.  `Path::exists` here checks the WASI sandbox
        // view, which is correct: the sandbox is the extension's work dir on
        // the host, so existence semantics match.
        if !std::path::Path::new(&inner_path).exists() {
            let zip_url = format!(
                "https://github.com/{ANNOTATIONS_REPO}/archive/refs/heads/{ANNOTATIONS_BRANCH}.zip"
            );

            zed::download_file(&zip_url, ANNOTATIONS_DIR, zed::DownloadedFileType::Zip)
                .map_err(|e| format!("Failed to download GMod annotations: {e}"))?;

            if !std::path::Path::new(&inner_path).exists() {
                return Err(format!(
                    "Annotations downloaded but expected path '{inner_path}' not found. \
                     The archive structure may have changed."
                ));
            }
        }

        // Resolve the absolute host path of the extension work dir.
        let work_dir = resolve_extension_work_dir(worktree)?;
        let absolute_path = format!("{work_dir}/{inner_path}");

        eprintln!(
            "[zed-glua] Resolved annotations path: {} (work_dir: {})",
            absolute_path, work_dir
        );

        self.cached_annotations_path = Some(absolute_path.clone());
        Ok(absolute_path)
    }
}

/// Reconstructs the absolute path of this extension's work directory on the
/// host.  Zed's extension API doesn't expose this directly, so we read the
/// user's shell environment via `worktree.shell_env()` and reproduce Zed's
/// well-known per-OS layout.
///
/// On Linux, Zed honours `$XDG_DATA_HOME` (defaulting to `~/.local/share`).
/// On macOS, Zed uses `~/Library/Application Support/Zed`.
/// On Windows, Zed uses `%APPDATA%/Zed` (or `%LOCALAPPDATA%`).
fn resolve_extension_work_dir(worktree: &Worktree) -> Result<String> {
    let env: std::collections::HashMap<String, String> =
        worktree.shell_env().into_iter().collect();

    let (os, _arch) = zed::current_platform();

    let zed_data_dir: String = match os {
        Os::Linux => {
            if let Some(xdg) = env.get("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
                format!("{xdg}/zed")
            } else if let Some(home) = env.get("HOME").filter(|s| !s.is_empty()) {
                format!("{home}/.local/share/zed")
            } else {
                return Err(
                    "Cannot determine Zed data dir: neither XDG_DATA_HOME nor HOME is set in the shell env"
                        .into(),
                );
            }
        }
        Os::Mac => {
            let home = env
                .get("HOME")
                .filter(|s| !s.is_empty())
                .ok_or("Cannot determine Zed data dir: HOME is not set in the shell env")?;
            format!("{home}/Library/Application Support/Zed")
        }
        Os::Windows => {
            let appdata = env
                .get("APPDATA")
                .or_else(|| env.get("LOCALAPPDATA"))
                .filter(|s| !s.is_empty())
                .ok_or("Cannot determine Zed data dir: neither APPDATA nor LOCALAPPDATA is set")?;
            // Normalize Windows backslashes so we can compose paths with `/`.
            format!("{}/Zed", appdata.replace('\\', "/"))
        }
    };

    Ok(format!("{zed_data_dir}/extensions/work/zed-glua"))
}

impl zed::Extension for GluaExtension {
    fn new() -> Self {
        GluaExtension {
            cached_binary_path: None,
            cached_annotations_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let binary = self.language_server_binary(language_server_id, worktree)?;

        // Allow the user to pass extra args via Zed LSP settings:
        //   "lsp": { "gmod-glua-ls": { "initialization_options": { ... } } }
        let settings = LspSettings::for_worktree("gmod-glua-ls", worktree)
            .ok()
            .and_then(|s| s.binary)
            .map(|b| b.arguments.unwrap_or_default())
            .unwrap_or_default();

        Ok(Command {
            command: binary,
            // glua_ls speaks LSP over stdio with no extra flags needed.
            args: settings,
            env: vec![],
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        _worktree: &Worktree,
    ) -> Result<Option<serde_json::Value>> {
        let mut opts = serde_json::Map::new();

        // Download annotations and pass the path so glua_ls knows about GMod
        // globals (CurTime, ParticleEmitter, Entity, etc.).  The path returned
        // is already an absolute host path, suitable for the LSP which runs
        // outside our WASI sandbox.
        match self.ensure_annotations(_worktree) {
            Ok(absolute_path) => {
                opts.insert(
                    "gmodAnnotationsPath".into(),
                    serde_json::Value::String(absolute_path),
                );
            }
            Err(e) => {
                // Non-fatal: the LSP will still work, just without GMod
                // globals.  Log the error so the user can see it.
                eprintln!("[zed-glua] Failed to set up annotations: {e}");
            }
        }

        // Merge any user-provided initialization_options on top so they can
        // override or add keys.
        if let Ok(settings) = LspSettings::for_worktree("gmod-glua-ls", _worktree) {
            if let Some(user_opts) = settings.initialization_options {
                if let serde_json::Value::Object(user_map) = user_opts {
                    for (k, v) in user_map {
                        opts.insert(k, v);
                    }
                }
            }
        }

        if opts.is_empty() {
            Ok(None)
        } else {
            Ok(Some(serde_json::Value::Object(opts)))
        }
    }
}

zed::register_extension!(GluaExtension);