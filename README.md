<h1 align="center">
  💼 rbxmonet 💎
</h1>

<div align="center">

[![License](https://img.shields.io/github/license/itzbbbbas/rbxmonet.svg?style=flat-square)](https://github.com/itzbbbbas/rbxmonet/blob/main/LICENSE.md)
[![Last Commit](https://img.shields.io/github/last-commit/itzbbbbas/rbxmonet.svg?style=flat-square)](https://github.com/itzbbbbas/rbxmonet/commits/main)

</div>

`rbxmonet` is a CLI for declaring, downloading, and syncing Roblox monetization (developer products, game passes, badges) into a single `rbxmonet.toml` file. It generates a `monets.luau` table the game can `require()` at runtime — and optionally a matching `.d.ts` for TypeScript projects.

Forked from [OutOfBears/rbx-products](https://github.com/OutOfBears/rbx-products). Differences:

- Renamed binary, package, and config file to `rbxmonet` / `rbxmonet.toml`.
- Single Open Cloud API key for **everything** (game passes, developer products, badges) — no cookie / `.ROBLOSECURITY` required.
- Full **Badges** support: list / create / update / icon upload via the `legacy-badges` + `legacy-publish` Open Cloud surfaces (mirrors [dev-bap/rbxsync](https://github.com/dev-bap/rbxsync)).
- Auto-prune deleted entries on `download` (stale `[gamepasses.*] / [products.*] / [badges.*]` blocks vanish when the remote id is gone; `id`-less pending creates are preserved).
- `icon = "path/to.png"` on any entry — uploaded as the asset icon on create + update (multipart, auto-resized + RGBA8 re-encoded).
- `[codegen]` block: configurable output path, `flat` or `nested` style, optional `.d.ts` sidecar, `[codegen.paths]` to rename/re-nest sections, `[codegen.extra]` to inject id leaves for assets rbxmonet doesn't track, and per-item `path = "..."` overrides.
- Per-section Luau + TypeScript export types (`Gamepass`, `Product`, `Badge`, `IdLeaf`).
- Slug-keyed Luau output (`["vip"]` instead of `["💲10% OFF💲 V.I.P"]`), formatted display in the `name` field.
- `regen-luau` subcommand — regenerate `monets.luau` from `rbxmonet.toml` offline (no network).
- `-a` / `--auto-confirm` flag (and `RBX_MONET_AUTO_CONFIRM` env) to skip the diff TUI on `sync`.
- Logging fix: errors visible without setting `RUST_LOG`.
- `ConfirmViewer` accepts `Y`/`y`/Enter and `N`/`n`/Esc.

---

## 📦 Install

Via [Rokit](https://github.com/rojo-rbx/rokit) (recommended for Roblox projects):

```toml
# rokit.toml
[tools]
rbxmonet = "itzbbbbas/rbxmonet@0.1.23"
```

```bash
rokit authenticate github --token <PAT_with_repo_scope>   # only if repo is private
rokit trust itzbbbbas/rbxmonet
rokit install
```

From source:

```bash
git clone https://github.com/itzbbbbas/rbxmonet.git
cd rbxmonet
cargo build --release
# Binary at target/release/rbxmonet(.exe)
```

Requires Rust **1.85+** (edition 2024). On Windows without MSVC toolchain, build with the GNU toolchain via MSYS2:

```powershell
winget install -e --id MSYS2.MSYS2
C:\msys64\usr\bin\pacman.exe -Sy --noconfirm mingw-w64-x86_64-toolchain
rustup toolchain install stable-x86_64-pc-windows-gnu
$env:PATH = "C:\msys64\mingw64\bin;" + $env:PATH
cargo +stable-x86_64-pc-windows-gnu build --release
```

---

## 🔐 Auth

`rbxmonet` uses a single Roblox Open Cloud API key for everything (game passes, developer products, badges, icon uploads).

```powershell
$env:RBX_MONET_API_KEY = "<your Open Cloud API key>"
```

Or drop into a `.env` next to `rbxmonet.toml`:

```
RBX_MONET_API_KEY=...
# RBX_MONET_AUTO_CONFIRM=true   # optional; skip diff viewer in `sync`
```

See `.env.example` for the canonical shape.

### Required API scopes

Configure these on the API key in the Creator Dashboard
([Open Cloud API Keys](https://create.roblox.com/dashboard/credentials)):

| Resource | Scopes | Documentation |
|---|---|---|
| Game Passes | `game-pass:read`, `game-pass:write` | [Game Passes API](https://create.roblox.com/docs/cloud/reference/GamePass) |
| Developer Products | `developer-product:read`, `developer-product:write` | [Developer Products API](https://create.roblox.com/docs/cloud/reference/DeveloperProduct) |
| Badges | `legacy-universe.badge:read`, `legacy-universe.badge:write`, `legacy-universe.badge:manage-and-spend-robux` | [Badges API](https://create.roblox.com/docs/cloud/legacy/badges/v1), [Universes — Badges](https://create.roblox.com/docs/cloud/reference/Badge) |
| Assets (icons) | `legacy-asset:manage` | [Assets](https://create.roblox.com/docs/cloud/reference/Asset) |

---

## 🚀 Commands

```
rbxmonet init           Create a starter rbxmonet.toml in the current directory
rbxmonet download       Pull universe products into rbxmonet.toml + regenerate monets.luau
rbxmonet sync           Push local changes to the universe; opens a TUI diff confirm
rbxmonet regen-luau     Regenerate the Luau file from rbxmonet.toml without any network calls
```

Global flags:

```
-y, --yes            Auto-confirm prompts
-o, --overwrite      On download, replace local values with remote values (does not preserve edits).
                     On sync, also skips the "upload non-existent products?" prompt and the diff viewer.
-a, --auto-confirm   In `sync`, skip the diff viewer and apply every diff (does NOT affect download semantics).
```

Resolution order for `--auto-confirm`: CLI flag > `RBX_MONET_AUTO_CONFIRM` env (`1` / `true` / `yes`, case-insensitive) > default (show diff viewer). Bare `rbxmonet sync` with neither shows the diff TUI like before.

---

## 🧩 `rbxmonet.toml` schema

```toml
[metadata]
universe-id     = 9946763161               # numeric Roblox universe id
discount-prefix = "💲{}% OFF💲 "           # {} is substituted with the discount percent
name-filters    = []                       # regex list applied to remote names on download

[codegen]
output     = "src/ReplicatedFirst/monets.luau"   # required; omit to skip Luau generation
# style    = "flat"                              # "flat" (default) or "nested"
# typescript = false                             # also emit <output>.d.ts sidecar

# ------- Game passes -------
[gamepasses.vip]                           # slug becomes the Luau key
id              = 1834607988               # written by `download`; you may leave it out for new entries
name            = "V.I.P"                  # internal name (display name = discount-prefix + name)
description     = "Daily perks"
price           = 199                      # Robux
active          = true
discount        = 10                       # 0–100; 0 disables the discount prefix
regional-pricing = true
icon            = "assets/icons/vip.png"   # optional; uploaded on create + update

# ------- Developer products -------
[products.starter-pack]
id              = 3554032826
name            = "Starter Pack"
description     = ""
price           = 199
active          = true
discount        = 0
regional-pricing = true
icon            = "assets/icons/starter.png"

# ------- Badges -------
[badges.first-win]
id              = 2147483648               # leave unset to have `sync` create the badge
name            = "First Win"
description     = "Awarded for your first victory"
active          = true                     # maps to badge `enabled`
icon            = "assets/icons/first-win.png"
```

### Slug keys

The header key (`vip` / `starter-pack` / `first-win`) is what shows up in the generated Luau table. Game code looks items up by this slug, not by display name:

```luau
monets.Gamepasses.vip.id   -- 1834607988
monets.Products["starter-pack"].price   -- 199
```

Slug syntax follows TOML bare-key rules: `[a-zA-Z0-9_-]`. Use quotes for anything outside that set:

```toml
[gamepasses.vip]              # bare — preferred
[gamepasses."v.i.p"]          # quoted — required when the slug contains "." or other reserved chars
```

### Discount prefix

When `discount > 0`, the generated `name` field in `monets.luau` becomes:

```
<discount-prefix with {} -> discount> + name
```

So `discount = 10` + `discount-prefix = "💲{}% OFF💲 "` + `name = "V.I.P"` →
`name = "💲10% OFF💲 V.I.P"` in the Luau output. The TOML stays clean (`name = "V.I.P"`).

### `[codegen]` — output structure

```toml
[codegen]
output     = "src/ReplicatedFirst/monets.luau"
# style    = "flat"           # "flat" (default) or "nested"
# typescript = false          # also emit <output>.d.ts sidecar
```

`style = "flat"` (default) emits each leaf at the root keyed by its dotted path. `style = "nested"` builds a tree of tables.

The three built-in sections default to `Gamepasses`, `Products`, `Badges`. Use `[codegen.paths]` to rename or re-nest them, `[codegen.extra]` to inject id leaves for assets rbxmonet doesn't track, and the per-item `path = "..."` field to relocate individual entries.

```toml
[codegen]

[codegen.paths]
passes   = "Gamepasses"           # default if omitted
products = "Products"
badges   = "Items.Badges"         # dot-separated → nested table

[codegen.extra]
"Gamepasses.legacy_vip" = 1234567 # emits `{ id = 1234567 }`

[gamepasses.vip]
id   = 1834607988
path = "Shop.Premium"             # per-item override; lands under Shop.Premium.vip
# ...
```

Output under default **flat** style:

```luau
return {
    ["Gamepasses.legacy_vip"] = { id = 1234567 },
    ["Gamepasses.vip"] = { id = 1834607988, price = 179, name = "V.I.P", description = "" },
    ["Items.Badges.first-win"] = { id = ..., price = 0, name = "First Win", description = "..." },
    ["Shop.Premium.vip"] = { id = 1834607988, price = 179, name = "V.I.P", description = "" },
}
```

Output under `style = "nested"`:

```luau
return {
    Gamepasses = {
        legacy_vip = { id = 1234567 },
        vip = { id = 1834607988, price = 179, name = "V.I.P", description = "" },
    },
    Items = {
        Badges = {
            ["first-win"] = { id = ..., price = 0, name = "First Win", description = "..." },
        },
    },
    Shop = {
        Premium = {
            vip = { id = 1834607988, price = 179, name = "V.I.P", description = "" },
        },
    },
}
```

When `typescript = true`, rbxmonet writes a `.d.ts` sidecar next to the `.luau` (variable name = file stem, sanitized to a valid TS identifier):

```ts
// monets.d.ts — auto-generated by rbxmonet. Do not edit.
export interface Gamepass { id: number; price: number; name: string; description: string }
export interface Product { id: number; price: number; name: string; description: string }
export interface Badge { id: number; price: number; name: string; description: string }
export interface IdLeaf { id: number }

declare const monets: {
    "Gamepasses.vip": Gamepass;
    "Gamepasses.legacy_vip": IdLeaf;
    "Items.Badges.first-win": Badge;
};
export default monets;
```

(nested style emits a tree shape matching the Luau output)

Leaf shape stays the rich `{ id, price, name, description }` table rbxmonet has always emitted; only `[codegen.extra]` entries are minimal `{ id = N }` since they carry no other fields. Sections are sorted alphabetically.

### `name-filters`

Regex patterns applied to remote names before deriving a slug on `download`. Empty array `[]` keeps the defaults:

```
💲.*?% OFF💲                 # strip discount prefix
\[.*?\]                     # strip bracketed text
[^a-zA-Z0-9!?,.\-\s]        # strip non-alphanumeric
```

Provide your own list to *replace* the defaults — they don't merge:

```toml
name-filters = [
    "💲.*?% OFF💲",
    "\\[.*?\\]",
    "[^a-zA-Z0-9!?,.\\-\\s]",
    "BETA",                  # also strip the word BETA
]
```

Backslashes must be escaped (TOML strings).

---

## 🧬 Generated `monets.luau`

```luau
-- This file is automatically generated by rbxmonet. Do not edit this file directly.
export type Gamepass = { id: number, price: number, name: string, description: string }
export type Product = { id: number, price: number, name: string, description: string }
export type Badge = { id: number, price: number, name: string, description: string }
export type IdLeaf = { id: number }

return {
    Gamepasses = {
        ["vip"] = { id = 1834607988, price = 199, name = "💲10% OFF💲 V.I.P", description = "Daily perks" }
    },

    Products = {
        ["starter-pack"] = { id = 3554032826, price = 199, name = "Starter Pack", description = "" }
    },

    Badges = {
        ["first-win"] = { id = 2147483648, price = 0, name = "First Win", description = "Awarded for your first victory" }
    }
}
```

Consume from game code:

```luau
local monets = require(game:GetService("ReplicatedFirst"):WaitForChild("monets"))

local vipId = monets.Gamepasses.vip.id
MarketplaceService:PromptGamePassPurchase(player, vipId)
```

---

## 🔄 Typical flows

**First-time setup for an existing universe**

```bash
rbxmonet init                            # creates a starter rbxmonet.toml
# edit [metadata].universe-id and [codegen].output
rbxmonet download                        # pulls every gamepass / product / badge
git add rbxmonet.toml src/ReplicatedFirst/monets.luau
```

**Add a new gamepass**

1. Add `[gamepasses.<slug>]` block to `rbxmonet.toml`, leave `id` unset (include `icon = "..."` if you want one on first push).
2. `rbxmonet sync` — confirm the upload prompt.
3. Roblox returns the new id, `rbxmonet` writes it back to the TOML.

**Change prices on existing items**

1. Edit `price = ...` in `rbxmonet.toml`.
2. `rbxmonet sync` — diff TUI opens. Press `c` to mark each item to push, `q` to leave the viewer, `y` at the confirm prompt. Or run `rbxmonet sync -a` to skip the TUI entirely.
3. `monets.luau` is regenerated automatically.

**No-network refresh of the Luau output after editing TOML**

```bash
rbxmonet regen-luau
```

**CI / unattended sync**

```bash
RBX_MONET_AUTO_CONFIRM=true rbxmonet sync
# or equivalently
rbxmonet sync -a
```

---

## 🖥️ Sync TUI keys

Inside the Diff viewer:

| Key       | Action                                                       |
|-----------|--------------------------------------------------------------|
| `↑` / `↓` | Navigate items                                               |
| `Enter`   | Open detail view (`q` to return to list)                     |
| `c`       | Mark / unmark the current item for sync                      |
| `C`       | Toggle "all items marked"                                    |
| `q`       | Exit viewer and continue to the confirm prompt               |

Confirm prompt:

| Key                | Action          |
|--------------------|-----------------|
| `y` / `Y` / Enter  | Accept (sync)   |
| `n` / `N` / Esc    | Cancel          |

`-a` / `--auto-confirm` skips both screens.

---

## 🧹 Auto-prune on `download`

`rbxmonet download` removes any local `[gamepasses.<slug>] / [products.<slug>] / [badges.<slug>]` whose `id` is no longer present in the remote universe. Pending creates (entries without an `id`) are preserved. If a section's fetch fails (auth error, 5xx), that section is skipped — never pruned to empty by accident.

When a prune happens, you'll see:

```
INFO pruned 1 gamepass entries no longer in remote: legacy-vip (use `git checkout rbxmonet.toml` to undo)
```

Git is the safety net.

---

## ⚠️ Limitations

- **Icon uploads** require the `legacy-asset:manage` scope on your API key. Without it, `sync` will create the entry but log a warning about the icon failing.
- **Badge create cost.** New badges in a universe consume free-quota first, then cost Robux per the `badges.roblox.com/v1/badges/metadata` `badgeCreationPrice` value. `sync` fetches the cost up-front and bails if the universe lacks balance.
- **Subscriptions are not supported.** Roblox Open Cloud does not expose create / update for subscription-products, and there's no stable list endpoint. Use the Creator Dashboard.
- **Per-entry `path = "..."`** lives in TOML and is preserved across `download`, but it is purely a local codegen concern — never reflected on Roblox.

---

## 🔧 Logging

By default the binary logs at `info` (release) and `debug` (debug builds), scoped to the `rbxmonet` crate. Override with `RUST_LOG`:

```powershell
$env:RUST_LOG = "debug"; rbxmonet sync
$env:RUST_LOG = "rbxmonet=trace"; rbxmonet download
```

Errors are also printed to stderr unconditionally as `error: <msg>`, regardless of `RUST_LOG`.

---

## 🧰 Troubleshooting

- **`error: TOML parse error ...`** — likely a `[products]` or `[gamepasses]` section without a slug. Use `[products.<slug>]`, never bare `[products]`.
- **`error: [metadata] luau-file is no longer supported — move to [codegen] output`** — migration from pre-v0.1.21 configs. Replace `[metadata] luau-file = "X"` with `[codegen] output = "X"`. Default style flipped to `flat` in v0.1.21 — if your game code reads `monets.Gamepasses.vip.id`, also add `style = "nested"`.
- **`401 / 403` from Roblox** — API key missing required scope. Re-issue the key with the scopes listed under **Auth**.
- **`HTTP 400 — "The badge icon is invalid."`** — only reported by the legacy `badges.roblox.com` write path; rbxmonet uses the Open Cloud `legacy-badges` surface, which accepts the same image. If you see this, you're on a pre-v0.1.17 build — upgrade.
- **`rokit install` fails with `no release was found`** — repo is private; run `rokit authenticate github --token <PAT>` once with a token that has `repo` scope.
- **Diff TUI marks items but nothing syncs** — after `q` exits the diff viewer, a second confirm prompt asks "Would you like to sync products?" — answer `y` there, or use `rbxmonet sync -a` to skip both.

---

## 🗒️ Changelog highlights

- **0.1.23** — `--auto-confirm` / `-a` flag + `RBX_MONET_AUTO_CONFIRM` env to skip the diff TUI on `sync`.
- **0.1.22** — Removed all `RBX_MONET_ROBLOSECURITY` cookie code; everything now flows through Open Cloud. Removed Subscriptions entirely (Roblox-side limitation). Added per-section Luau + TS export types (`Gamepass`, `Product`, `Badge`, `IdLeaf`).
- **0.1.21** — Moved output path into `[codegen] output`. Added `style = "flat" | "nested"` (flat default). Added `typescript = true` for `.d.ts` sidecar emission.
- **0.1.20** — Added `[codegen]`, `[codegen.paths]`, `[codegen.extra]`, and per-item `path = "..."` overrides.
- **0.1.19** — Auto-prune deleted entries on `download` (with guards for `id`-less pending creates and skipped sections).
- **0.1.18** — `icon = "..."` field on game passes + dev products (renamed from `icon-file`).
- **0.1.17** — Ported badge create / update / icon to the `legacy-badges` + `legacy-publish` Open Cloud surfaces (mirrors rbxsync). Fixes "badge icon is invalid" 400.
- **0.1.16** — Two-client host-bucket split (Open Cloud vs legacy) and subscription read endpoint wiring (later removed in 0.1.22).
- **0.1.15** — `.ROBLOSECURITY` cookie support (later removed in 0.1.22) and `RBX_MONET_*` env-var rename.

---

## 📄 License

MIT — same as upstream [OutOfBears/rbx-products](https://github.com/OutOfBears/rbx-products/blob/main/LICENSE.md).
