<h1 align="center">
  💼 rbxmonet 💎
</h1>

<div align="center">

[![License](https://img.shields.io/github/license/itzbbbbas/rbxmonet.svg?style=flat-square)](https://github.com/itzbbbbas/rbxmonet/blob/main/LICENSE.md)
[![Last Commit](https://img.shields.io/github/last-commit/itzbbbbas/rbxmonet.svg?style=flat-square)](https://github.com/itzbbbbas/rbxmonet/commits/main)

</div>

`rbxmonet` is a CLI for declaring, downloading, and syncing Roblox monetization (developer products, game passes, subscriptions) into a single `rbxmonet.toml` file. It generates a `monets.luau` table the game can `require()` at runtime.

Forked from [OutOfBears/rbx-products](https://github.com/OutOfBears/rbx-products). Differences:

- Renamed binary, package, and config file to `rbxmonet` / `rbxmonet.toml`.
- Slug-keyed Luau output (`["vip"]` instead of `["💲10% OFF💲 V.I.P"]`), formatted display in the `name` field.
- `name` + `description` fields emitted per entry.
- Read-only **Subscriptions** support (Open Cloud has no create/update for subscription-products).
- `regen-luau` subcommand — regenerate `monets.luau` from `rbxmonet.toml` offline (no network).
- Logging fix: errors visible without setting `RUST_LOG`.
- `ConfirmViewer` accepts `Y`/`y`/Enter and `N`/`n`/Esc.

---

## 📦 Install

Via [Rokit](https://github.com/rojo-rbx/rokit) (recommended for Roblox projects):

```toml
# rokit.toml
[tools]
rbxmonet = "itzbbbbas/rbxmonet@0.1.9"
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

`rbxmonet sync` and `rbxmonet download` need a Roblox Open Cloud API key.

Set `RBX_API_KEY`:

```powershell
$env:RBX_API_KEY = "<your Open Cloud API key>"
```

Or drop it in `.env` next to `rbxmonet.toml`:

```
RBX_API_KEY=...
```

**Scopes** (configure on the API key in the Creator Dashboard):

| Feature       | Required scopes                                                                 |
|---------------|---------------------------------------------------------------------------------|
| Game passes   | `game-pass:read`, `game-pass:write`                                             |
| Dev products  | `developer-product:read`, `developer-product:write`                             |
| Subscriptions | `universe.subscription-product.subscription:read` (read only — see Limitations) |
| Badges        | `legacy-universe.badge:write`, `legacy-badge:manage` (update only — see Limitations) |

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
-y, --yes         Auto-confirm prompts
-o, --overwrite   On download, replace local values with remote values (does not preserve edits)
```

---

## 🧩 `rbxmonet.toml` schema

```toml
[metadata]
universe-id     = 9946763161               # numeric Roblox universe id
luau-file       = "src/ReplicatedFirst/monets.luau"   # optional; omit to skip Luau generation
discount-prefix = "💲{}% OFF💲 "           # {} is substituted with the discount percent
name-filters    = []                       # regex list applied to remote names on download

# ------- Game passes (full CRUD via API) -------
[gamepasses.vip]                           # slug becomes the Luau key
id              = 1834607988               # written by `download`; you may leave it out for new entries
name            = "V.I.P"                  # internal name (display name = name + discount-prefix)
description     = "Daily perks"
price           = 199                      # Robux
active          = true
discount        = 10                       # 0–100; 0 disables the discount prefix
regional-pricing = true

# ------- Developer products (full CRUD via API) -------
[products.starter-pack]
id              = 3554032826
name            = "Starter Pack"
description     = ""
price           = 199
active          = true
discount        = 0
regional-pricing = true

# ------- Subscriptions (read only via API; id is the full "EXP-..." string) -------
[subscriptions.gold-tier]
id              = "EXP-11111111"           # full subscription product id (must include EXP- prefix; required by MarketplaceService)
name            = "Gold Tier"
description     = "Monthly gold perks"
price           = 99                       # Robux
active          = true

# ------- Badges (download + update name/description/enabled via API; create requires icon upload) -------
[badges.first-win]
id              = 2147483648
name            = "First Win"
description     = "Awarded for your first victory"
active          = true                     # maps to badge `enabled`
```

### Slug keys

The header key (`vip` / `starter-pack` / `gold-tier`) is what shows up in the generated Luau table. Game code looks items up by this slug, not by display name:

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
export type Product = { id: number, price: number, name: string, description: string }
export type Subscription = { id: string, price: number, name: string, description: string }

return {
    Gamepasses = {
        ["vip"] = { id = 1834607988, price = 199, name = "💲10% OFF💲 V.I.P", description = "Daily perks" }
    },

    Products = {
        ["starter-pack"] = { id = 3554032826, price = 199, name = "Starter Pack", description = "" }
    },

    Subscriptions = {
        ["gold-tier"] = { id = "EXP-11111111", price = 99, name = "Gold Tier", description = "Monthly gold perks" }
    },

    Badges = {
        ["first-win"] = { id = 2147483648, price = 0, name = "First Win", description = "Awarded for your first victory" }
    }
}
```

Note: subscription ids are **strings** (`"EXP-..."`) — Roblox engine APIs like `MarketplaceService:PromptSubscriptionPurchase` require the full prefixed form.

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
# edit [metadata].universe-id
rbxmonet download                        # pulls every gamepass/product/subscription
git add rbxmonet.toml src/ReplicatedFirst/monets.luau
```

**Add a new gamepass**

1. Add `[gamepasses.<slug>]` block to `rbxmonet.toml`, leave `id` unset.
2. `rbxmonet sync` — confirm the upload prompt.
3. Roblox returns the new id, `rbxmonet` writes it back to the TOML.

**Change prices on existing items**

1. Edit `price = ...` in `rbxmonet.toml`.
2. `rbxmonet sync` — diff TUI opens. Press `c` to mark each item to push, `q` to leave the viewer, `y` at the confirm prompt.
3. `monets.luau` is regenerated automatically.

**No-network refresh of the Luau output after editing TOML**

```bash
rbxmonet regen-luau
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

---

## ⚠️ Limitations

- **Subscriptions are read-only.** Roblox Open Cloud does not expose create/update for subscription-products as of 2026-05. Create them in the Creator Dashboard, then run `rbxmonet download` to pull the new id into `rbxmonet.toml`.
- **Badges support download + update only.** Create requires an icon file upload that is not wired yet — create the badge in the Creator Dashboard, then run `rbxmonet download`. `rbxmonet sync` will PATCH `name`, `description`, and `enabled` for existing badges.
- **Icon assets:** game-pass and dev-product icon upload is not yet wired.
- **Subscriptions table when `luau-file` is set on a universe with no subscriptions:** an empty `Subscriptions = {}` block is emitted.

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
- **`401 / 403` from Roblox** — API key missing required scope. Re-issue the key with the scopes listed under **Auth**.
- **`subscription-products endpoint returned 404 / 401 / 403`** — your API key lacks the subscription read scope, or your universe has no subscriptions. The rest of `download` continues normally.
- **`rokit install` fails with `no release was found`** — repo is private; run `rokit authenticate github --token <PAT>` once with a token that has `repo` scope.
- **Diff TUI marks items but nothing syncs** — after `q` exits the diff viewer, a second confirm prompt asks "Would you like to sync products?" — answer `y` there.

---

## 📄 License

MIT — same as upstream [OutOfBears/rbx-products](https://github.com/OutOfBears/rbx-products/blob/main/LICENSE.md).
