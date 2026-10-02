# Garry's Mod Lua Support for Zed

Adds first-class support for Garry's Mod's Lua variant (GLua) to the [Zed
editor](https://zed.dev), backed by Pollux12's
[`gmod-glua-ls`](https://github.com/Pollux12/gmod-glua-ls) language server.

> **Status:** preview. The core editing experience works well; some
> VSCode-only features (live debugger, entity explorer, MCP host) are not
> available in Zed.

## Features

- Syntax highlighting for standard Lua **and** GLua-specific additions
  (`continue`, `&&`/`||`/`!=`/`!` C-style operators, GMod global types).
- Full Language Server Protocol integration via `gmod-glua-ls`:
  - Hover docs, go-to-definition, find references, rename.
  - Diagnostics, including realm-aware checks (`sv_*`, `cl_*`, `sh_*`
    prefixes and `include()` chains).
  - Autocomplete for all GMod globals, methods, and hooks.
  - Network library validation (`net.Start` / `net.Receive` matching).
  - VGUI panel awareness and scripted class (`ENT`, `SWEP`, `TOOL`, ...)
    resolution.
- Automatic download of GMod wiki annotations on first launch — no manual
  setup required.

## Requirements

- Zed (latest stable). The extension is built against
  `zed_extension_api = 0.4`.
- An internet connection on first launch so the extension can download:
  - The platform-appropriate `glua_ls` binary from the
    [`Pollux12/gmod-glua-ls`](https://github.com/Pollux12/gmod-glua-ls/releases)
    release page (~30 MB).
  - The GMod annotations from the
    [`gmod-luals-addon`](https://github.com/Pollux12/gmod-luals-addon)
    repo's `gluals-annotations` branch (~20 MB).
- Supported platforms (as published by the upstream LSP CI):
  - Linux x86_64
  - Windows x86_64
  - macOS is **not currently supported** by the upstream binary releases.

## Installation

### From the extension gallery

1. Open the command palette (`cmd-shift-p` / `ctrl-shift-p`).
2. Run `zed: extensions`.
3. Search for **"Garry's Mod Lua Support"** and click **Install**.

### From source (dev install)

```sh
git clone https://github.com/vaqxai/zed-glua
```

Then in Zed:

1. `zed: extensions` → **Install Dev Extension**.
2. Point it at the cloned `zed-glua` directory.

Zed will compile the extension to wasm and install it locally.

## First launch

The first time you open a `.lua` file with GLua selected, the extension will:

1. Download the appropriate `glua_ls` release for your platform.
2. Download the GMod annotation bundle.
3. Start the language server and stream initial diagnostics.

Expect a one-time pause of a few seconds while these downloads happen. They
land in:

- **Linux:** `~/.local/share/zed/extensions/work/zed-glua/`
- **macOS:** `~/Library/Application Support/Zed/extensions/work/zed-glua/`
- **Windows:** `%APPDATA%\Zed\extensions\work\zed-glua\`

Subsequent launches are instant and offline-capable.

## Getting GLua applied to `.lua` files

The extension registers `GLua` as a language for `.lua` files. If Zed routes
`.lua` to its built-in `Lua` language instead, add this to your
`settings.json`:

```json
{
  "file_types": {
    "GLua": ["lua"]
  }
}
```

You can verify the active language by clicking the indicator in the bottom
status bar, it should read **GLua**, not **Lua**.

## Configuration

The language server reads `.gluarc.json` from your project root (same format
as VSCode). A minimal example:

```json
{
  "$schema": "https://raw.githubusercontent.com/Pollux12/gmod-glua-ls/main/crates/glua_code_analysis/resources/schema.json",
  "workspace": {
    "library": [
      "/path/to/extra/annotations"
    ]
  },
  "diagnostics": {
    "severity": {
      "undefined-global": "warning"
    }
  }
}
```

Full schema reference:
<https://gluals.arnux.net/configuration/overview>.

### Sharing globals across workspace folders

Zed starts a **separate LSP instance per workspace folder**. If you have two
directories added to the same Zed workspace (e.g. your addon and a shared
library like `zclib`), the addon's LSP will not know about globals defined in
the library folder by default.

There are two ways to fix this.

#### Option A — per-project `.gluarc.json` (recommended)

Add a `.gluarc.json` to the project that *uses* the shared library and list
the library folder under `workspace.library`:

```json
{
  "$schema": "https://raw.githubusercontent.com/Pollux12/gmod-glua-ls/main/crates/glua_code_analysis/resources/schema.json",
  "workspace": {
    "library": ["C:/path/to/zclib"]
  }
}
```

The LSP will index the library folder as read-only reference code and expose
all its globals to the current project's files.

#### Option B — global Zed `settings.json`

If you want the library paths to apply to **every** GLua project you open
(without touching individual `.gluarc.json` files), add them to your Zed
`settings.json` under `lsp.gmod-glua-ls.initialization_options.workspace.library`:

```json
{
  "lsp": {
    "gmod-glua-ls": {
      "initialization_options": {
        "workspace": {
          "library": [
            "C:/path/to/zclib",
            "C:/path/to/another-shared-lib"
          ]
        }
      }
    }
  }
}
```

The extension automatically **deep-merges** these paths with any library
paths it injects itself (e.g. the GMod annotations), so nothing is
overwritten.

### LSP settings via Zed

You can pass extra args or initialization options to `glua_ls` from your
Zed `settings.json`:

```json
{
  "lsp": {
    "gmod-glua-ls": {
      "binary": {
        "arguments": ["--log-level", "debug"]
      },
      "initialization_options": {
        "anything-glua-ls-accepts": "..."
      }
    }
  }
}
```

## Limitations vs. the VSCode extension

The following VSCode features are **not** available in Zed:

- The live debugger (gm_rdb attach/launch).
- The Entity Explorer and Class Explorer side panels.
- MCP host for AI tool integration.
- Run Lua / Run File / Refresh File commands.
- The `.gluarc.json` settings UI (edit the JSON directly).

The core editing experience (highlighting, diagnostics, completion, hover,
navigation, formatting) is at parity.

## Troubleshooting

### "GLua" doesn't appear in the language picker

If the language is missing entirely, the extension's wasm build or its
tree-sitter query files probably failed to load. Check the Zed log:

```sh
# Linux / macOS
tail -f ~/.local/share/zed/logs/Zed.log

# or open it from Zed
```

…and look for lines like:

```
ERROR [language::language_registry] failed to load language GLua:
Error loading highlights query
```

If the wasm build itself failed, the failure shows up earlier in the log
under `extension::extension_builder`. Rebuild with
`zed: rebuild dev extension`.

### LSP starts but GMod globals are not recognized

(e.g. hovering `Angle` says "undefined global variable")

This means the annotations aren't loaded. Verify by checking the LSP's own
log file:

- **Linux:** `~/.local/share/gmod_glua_ls/logs/`
- **macOS:** `~/Library/Application Support/gmod_glua_ls/logs/`
- **Windows:** `%LOCALAPPDATA%\gmod_glua_ls\logs\`

Look for a line like:

```
[INFO glua_ls::handlers::initialized] Received gmodAnnotationsPath from VSCode: /home/.../zed-glua/gmod-annotations/...
```

(The "from VSCode" string is hardcoded in the LSP — it doesn't actually mean
the path came from VSCode.)

If that line is missing or the path is `None`, the extension failed to
resolve an absolute path to the annotations directory. See **"On Zed
updates"** below for the most likely cause and remediation.

### Diagnostics are stale or wrong

Run `zed: restart language server`. If that doesn't help, delete the
extension's work directory (path shown under **First launch** above) and
restart Zed — this forces a clean re-download.

### Stdlib (`math.deg`, `string.upper`) works but GMod globals don't

This is the same problem as **"LSP starts but GMod globals are not
recognized"**. Stdlib annotations ship inside the `glua_ls` binary and are
loaded via a separate mechanism; the GMod annotations come from the
downloaded zip and require the absolute-path workaround described below.

## On Zed updates (the fragile bit)

⚠️ **The annotations download mechanism depends on Zed's internal directory
layout.** Zed currently does not expose, via its extension API, the
absolute host path of an extension's working directory. To pass the GMod
annotation files to the language server (which runs outside the WASI
sandbox), this extension reconstructs that path by combining:

- `worktree.shell_env()["HOME"]` (or `XDG_DATA_HOME` / `APPDATA` /
  `LOCALAPPDATA` depending on OS), with
- Zed's well-known per-platform extension storage location.

If a future Zed release changes where it stores extension work
directories, the path reconstruction will break, the language server will
silently fail to find the annotations, and you'll be back to "globals not
recognized."

**Symptoms of breakage after a Zed update:**

- Lua stdlib still works (hover on `math.pi`, `string.format`, etc.).
- Every GMod global (`Angle`, `Vector`, `ents`, `hook`, ...) shows as
  undefined.
- The LSP log shows the annotation path the extension sent, but the path
  does not exist on disk.

**Mitigations:**

1. **Open an issue** on this repository with the failing path from the
   LSP log and the actual location of your `zed-glua` work directory.
2. **Workaround**: set `gluals.ls.annotationPath` in a `.gluarc.json` to
   the actual on-disk location of the annotations directory.
3. **Track upstream**: ideally Zed would expose
   `Worktree::extension_work_dir()` or similar; if you're a Zed user
   reading this, upvoting or filing such a feature request helps.

## Credits

- [Pollux12](https://github.com/Pollux12) — author of the `gmod-glua-ls`
  language server (a hard fork of EmmyLuaAnalyzer specialised for GLua)
  and the original [VSCode
  extension](https://github.com/Pollux12/vscode-gmod-glua-ls).
- [`tree-sitter-lua`](https://github.com/tree-sitter-grammars/tree-sitter-lua)
  — the grammar this extension builds on.
- The Garry's Mod community for the
  [wiki](https://wiki.facepunch.com/gmod) content that powers the
  annotations.

## License

MIT.
