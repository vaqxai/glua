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
            // Zed stores its data under %LOCALAPPDATA%\Zed on Windows
            // (e.g. C:\Users\<user>\AppData\Local\Zed), NOT %APPDATA%\Zed
            // (which would be the Roaming folder). We must prefer
            // LOCALAPPDATA; APPDATA is only useful as a last resort, and
            // even then we have to rewrite "…/Roaming" → "…/Local" because
            // the Roaming Zed dir does not exist on disk.
            let raw = env
                .get("LOCALAPPDATA")
                .filter(|s| !s.is_empty())
                .map(|s| format!("{s}/Zed"))
                .or_else(|| {
                    // Derive %LOCALAPPDATA% from %USERPROFILE% if the shell
                    // env didn't propagate LOCALAPPDATA itself.
                    env.get("USERPROFILE")
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("{s}/AppData/Local/Zed"))
                })
                .or_else(|| {
                    // Last-resort: rewrite APPDATA's trailing "Roaming"
                    // segment to "Local".
                    env.get("APPDATA")
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            let normalized = s.replace('\\', "/");
                            if let Some(stripped) = normalized
                                .strip_suffix("/Roaming")
                                .or_else(|| normalized.strip_suffix("/roaming"))
                            {
                                format!("{stripped}/Local/Zed")
                            } else {
                                format!("{normalized}/Zed")
                            }
                        })
                })
                .ok_or(
                    "Cannot determine Zed data dir on Windows: none of LOCALAPPDATA, USERPROFILE, or APPDATA is set in the shell env",
                )?;
            // Normalize Windows backslashes so we can compose paths with `/`.
            raw.replace('\\', "/")
        }
    };

    Ok(format!("{zed_data_dir}/extensions/work/zed-glua"))
}

/// Deep-merges a user-supplied `workspace` JSON object into `opts`.
///
/// Arrays (e.g. `workspace.library`) are *unioned* (user entries appended
/// after any existing entries) rather than replaced, so that the extension's
/// own entries are never lost.  All other scalar / object keys inside
/// `workspace` are inserted with the user's value taking precedence.
fn deep_merge_workspace(
    opts: &mut serde_json::Map<String, serde_json::Value>,
    user_workspace: serde_json::Value,
) {
    let serde_json::Value::Object(user_ws_map) = user_workspace else {
        // If it's not an object, just overwrite wholesale.
        opts.insert("workspace".into(), user_workspace);
        return;
    };

    // Obtain or create the "workspace" object in opts.
    let existing_ws = opts
        .entry("workspace")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    let serde_json::Value::Object(ref mut existing_map) = *existing_ws else {
        // Existing value is not an object – overwrite.
        *existing_ws = serde_json::Value::Object(user_ws_map);
        return;
    };

    for (k, v) in user_ws_map {
        match (existing_map.get_mut(&k), &v) {
            // Union arrays so neither side's entries are lost.
            (Some(serde_json::Value::Array(existing_arr)), serde_json::Value::Array(user_arr)) => {
                for item in user_arr {
                    if !existing_arr.contains(item) {
                        existing_arr.push(item.clone());
                    }
                }
            }
            // For everything else the user value wins.
            _ => {
                existing_map.insert(k, v);
            }
        }
    }
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
        // Log every LSP spawn so we can see in the Zed log which worktrees
        // are triggering new glua_ls instances (including "Unknown worktree"
        // entries that appear in the LSP Logs tab).
        eprintln!(
            "[zed-glua] language_server_command called | id={} | root='{}'",
            language_server_id,
            worktree.root_path()
        );

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

        // Merge any user-provided initialization_options on top.
        //
        // Special handling for `workspace.library`:
        //   glua_ls reads `workspace.library` from `.gluarc.json` on disk, but
        //   it also accepts it via init options.  Users who have GLua projects
        //   spread across multiple on-disk directories (e.g. an addon that
        //   depends on a shared library like zclib kept in a separate workspace
        //   folder) can list those extra paths in their Zed `settings.json`:
        //
        //   "lsp": {
        //     "gmod-glua-ls": {
        //       "initialization_options": {
        //         "workspace": {
        //           "library": ["C:/path/to/zclib", "C:/path/to/other-lib"]
        //         }
        //       }
        //     }
        //   }
        //
        //   The extension deep-merges `workspace.library` arrays so that any
        //   paths already present in `opts` (e.g. coming from an extension
        //   feature in the future) are preserved alongside the user's entries.
        if let Ok(settings) = LspSettings::for_worktree("gmod-glua-ls", _worktree) {
            if let Some(user_opts) = settings.initialization_options {
                if let serde_json::Value::Object(user_map) = user_opts {
                    for (k, v) in user_map {
                        if k == "workspace" {
                            // Deep-merge the "workspace" object so that
                            // workspace.library entries are unioned rather than
                            // overwritten wholesale.
                            deep_merge_workspace(&mut opts, v);
                        } else {
                            opts.insert(k, v);
                        }
                    }
                }
            }
        }

        // Log the final init options so users can diagnose configuration
        // problems (e.g. missing workspace.library paths) by looking at the
        // Zed log file.
        eprintln!(
            "[zed-glua] worktree root  : {}",
            _worktree.root_path()
        );
        eprintln!(
            "[zed-glua] init_options   : {}",
            serde_json::to_string(&opts).unwrap_or_else(|_| "<serialization error>".into())
        );

        if opts.is_empty() {
            Ok(None)
        } else {
            Ok(Some(serde_json::Value::Object(opts)))
        }
    }

    /// Called by Zed whenever glua_ls fires a `workspace/configuration` request.
    ///
    /// `glua_ls` receives its full configuration (workspace.library, diagnostics,
    /// etc.) through this channel — NOT by reading `.gluarc.json` from disk
    /// itself.  Without this method the LSP gets `{}` and ignores everything in
    /// `.gluarc.json`, including `workspace.library`, which is why globals from
    /// other workspace folders were never visible.
    ///
    /// This implementation:
    ///  1. Reads `.gluarc.json` / `.luarc.json` / `.emmyrc.json` from the
    ///     worktree root (whichever exists first).
    ///  2. Injects `gmod.annotationsPath` so the LSP always knows where the
    ///     downloaded GMod wiki annotations live, even without a config file.
    ///  3. Merges any `workspace.library` paths the user supplied via Zed
    ///     LSP settings (`initialization_options.workspace.library`) on top,
    ///     so both approaches work simultaneously.
    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<serde_json::Value>> {
        // 1. Load on-disk config file (first one that exists wins).
        let mut cfg: serde_json::Map<String, serde_json::Value> = {
            let names = [".gluarc.json", ".luarc.json", ".emmyrc.json"];
            let mut found: Option<serde_json::Map<String, serde_json::Value>> = None;
            for name in &names {
                match worktree.read_text_file(name) {
                    Ok(text) => {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(serde_json::Value::Object(m)) => {
                                eprintln!("[zed-glua] read config file   : {name}");
                                found = Some(m);
                                break;
                            }
                            Ok(_) => {
                                eprintln!("[zed-glua] config file not object: {name}");
                            }
                            Err(e) => {
                                eprintln!("[zed-glua] config file parse error {name}: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[zed-glua] read_text_file({name}) failed: {e}");
                    }
                }
            }
            found.unwrap_or_default()
        };

        // 2. Inject gmod.annotationsPath so the LSP finds the downloaded
        //    GMod wiki annotations regardless of whether the user has a
        //    config file.  Don't overwrite an explicit user value.
        if let Ok(abs_path) = self.ensure_annotations(worktree) {
            let gmod_entry = cfg
                .entry("gmod")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if let serde_json::Value::Object(gmod_map) = gmod_entry {
                gmod_map
                    .entry("annotationsPath")
                    .or_insert_with(|| serde_json::Value::String(abs_path));
            }
        }

        // 3. Merge workspace.library paths from Zed LSP settings on top of
        //    whatever the config file already specifies.
        if let Ok(settings) = LspSettings::for_worktree("gmod-glua-ls", worktree) {
            if let Some(user_opts) = settings.initialization_options {
                if let serde_json::Value::Object(user_map) = user_opts {
                    if let Some(user_ws) = user_map.get("workspace") {
                        deep_merge_workspace(&mut cfg, user_ws.clone());
                    }
                }
            }
        }

        eprintln!(
            "[zed-glua] workspace_cfg  : {}",
            serde_json::to_string(&cfg).unwrap_or_else(|_| "<serialization error>".into())
        );

        if cfg.is_empty() {
            Ok(None)
        } else {
            Ok(Some(serde_json::Value::Object(cfg)))
        }
    }
}

zed::register_extension!(GluaExtension);