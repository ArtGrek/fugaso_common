# Project Overview
- Rust workspace with multiple crates defined in the root `Cargo.toml` members list: `fugaso_service`, `fugaso_admin`, `fugaso_math`, `fugaso_core`, `fugaso_data`, `fugaso_config`, `fugaso_test`, `fugaso_math_ed6`, `fugaso_math_ed7`.【F:Cargo.toml†L1-L14】
- `fugaso_service` is the executable entry point and calls `fugaso_admin::config::run_server` in `main`.【F:fugaso_service/src/main.rs†L1-L6】
- `fugaso_admin` is the server/admin layer (dispatcher, routes, config, manager, etc.). Its public modules are declared in `src/lib.rs`.【F:fugaso_admin/src/lib.rs†L1-L7】
- `fugaso_core` provides core game administration logic, proxy integration, protocol types, and tournament integration (`admin`, `proxy`, `protocol`, `tournament`).【F:fugaso_core/src/lib.rs†L1-L4】
- `fugaso_math` defines slot-math traits, protocol types, FSM, validators, config, and RNG abstractions used by game math implementations.【F:fugaso_math/src/lib.rs†L1-L7】
- `fugaso_math_ed6` and `fugaso_math_ed7` contain concrete math implementations for games (Thunder Express and Mega Thunder) and implement `SlotMath`.【F:fugaso_math_ed6/src/lib.rs†L1-L4】【F:fugaso_math_ed7/src/lib.rs†L1-L4】
- `fugaso_data` contains SeaORM models and repositories for game data (`fugaso_action`, `fugaso_round`, etc.).【F:fugaso_data/src/lib.rs†L1-L13】
- `fugaso_config` embeds static config (`resources/bets.json`).【F:fugaso_config/src/lib.rs†L1-L1】

# Architecture
- **Infrastructure / integration layers**
  - HTTP/server dispatch and orchestration live in `fugaso_admin` (dispatcher, routes, manager, cache). The dispatcher wires requests to core admin and proxy layers.【F:fugaso_admin/src/lib.rs†L1-L7】【F:fugaso_admin/src/dispatcher.rs†L700-L940】
  - `fugaso_core::proxy` encapsulates account/jackpot/promo service interactions and is used by the dispatcher and admin logic.【F:fugaso_core/src/proxy.rs†L1-L7】【F:fugaso_admin/src/dispatcher.rs†L700-L940】
  - `fugaso_data` contains database models and repositories; it owns the SeaORM entities and models for actions/rounds and related tables.【F:fugaso_data/src/lib.rs†L1-L13】
- **Business logic layers**
  - `fugaso_core::admin::SlotAdmin` orchestrates round lifecycle and calls `SlotMath` methods (`join`, `init`, `spin`, `respin`, `free_spin`, `collect`, `close`).【F:fugaso_core/src/admin.rs†L290-L760】
  - `fugaso_math` defines the core math traits (`SlotMath`, `IPlayResponse`) and protocol data structures (`GameData`, `SpinData`, `GameResult`, etc.).【F:fugaso_math/src/math.rs†L16-L140】【F:fugaso_math/src/protocol.rs†L24-L210】
  - Concrete game math lives in `fugaso_math_ed6::math::ThunderExpressMath` and `fugaso_math_ed7::math::MegaThunderMath`, both implementing `SlotMath`.【F:fugaso_math_ed6/src/math.rs†L17-L120】【F:fugaso_math_ed6/src/math.rs†L349-L370】【F:fugaso_math_ed7/src/math.rs†L17-L120】【F:fugaso_math_ed7/src/math.rs†L356-L378】
- **External crates**
  - The workspace depends on external `essential_*` crates (e.g., `essential_core`, `essential_async`, `essential_rand`) and `sea-orm`. Their behavior is outside this repository; do not infer details beyond usage sites in this codebase.【F:Cargo.toml†L16-L35】

# Key Concepts & Types
- **`SlotMath` trait** (core game math interface): defined in `fugaso_math/src/math.rs` with required methods `join`, `init`, `spin`, `free_spin`, `respin`, `post_process`, `close`, `collect`, plus configuration/validator types and RNG setup.【F:fugaso_math/src/math.rs†L94-L140】
  - `SlotBaseMath` provides a wrapper interface that forwards to a parent `SlotMath` implementation and itself implements `SlotMath` via delegation.【F:fugaso_math/src/math.rs†L142-L227】
  - `ProxyMath` and `ReplayMath` are wrappers around `SlotMath` for controlled execution/replay scenarios.【F:fugaso_math/src/math.rs†L491-L714】
  - Concrete implementations:
    - `ThunderExpressMath<R>` implements `SlotMath` in `fugaso_math_ed6/src/math.rs`.【F:fugaso_math_ed6/src/math.rs†L349-L370】
    - `MegaThunderMath<R>` implements `SlotMath` in `fugaso_math_ed7/src/math.rs`.【F:fugaso_math_ed7/src/math.rs†L356-L378】
- **Action types**
  - `fugaso_action::ActionKind` enumerates all action states (`BET`, `SPIN`, `RESPIN`, `FREE_SPIN`, `COLLECT`, etc.).【F:fugaso_data/src/fugaso_action.rs†L18-L71】
  - `fugaso_action::Model` is the SeaORM entity for persisted actions; it includes `act_descr`, `next_act`, bet and result fields, etc.【F:fugaso_data/src/fugaso_action.rs†L72-L130】
- **Initialization and request arguments** (math side)
  - `JoinArg` (fields: balance, round info, current bet/lines/reels, possible settings, promo state).【F:fugaso_math/src/math.rs†L52-L72】
  - `GameInitArg` (round context and current bet/lines/reels used for restore/init).【F:fugaso_math/src/math.rs†L74-L86】
  - `SpinArg` (balance, round info, next action, promo, stake) is passed to `spin`/`respin`/`free_spin`/`collect`.【F:fugaso_math/src/math.rs†L87-L93】
- **Protocol data structures**
  - `GameData` is the main response enum: `Initial`, `Spin`, `ReSpin`, `Collect`, `FreeSpin`.【F:fugaso_math/src/protocol.rs†L24-L53】
  - `SpinData` is the core payload for spin-like actions and carries result, round info, promo, and free-game state.【F:fugaso_math/src/protocol.rs†L260-L275】
  - `GameResult` stores the grid/stops/special data and wins (`Gain`).【F:fugaso_math/src/protocol.rs†L375-L403】

# Data Flow
- **Login/Join flow**
  - Dispatcher login/registration leads to `Dispatcher::join`, which initializes the admin with `InitArg`, then calls proxy `join()` and admin `join()` to return initial `GameData` packets.【F:fugaso_admin/src/dispatcher.rs†L700-L780】
  - `SlotAdmin::join` builds a `JoinArg` and calls `SlotMath::join`, returning `GameData::Initial` or other `GameData` variants depending on the math implementation.【F:fugaso_core/src/admin.rs†L290-L327】
- **Restore/init flow**
  - If prior round/actions are found, `SlotAdmin::restore` calls `SlotMath::init` with `GameInitArg` and the stored `fugaso_action::Model` list to restore math state.【F:fugaso_core/src/admin.rs†L330-L401】
- **Spin flow**
  - Dispatcher `on_spin` calls `SlotAdmin::spin`, which validates input, records rounds/actions, and invokes `SlotMath::spin` with `SpinArg` and the validator step/combo context.【F:fugaso_admin/src/dispatcher.rs†L844-L882】【F:fugaso_core/src/admin.rs†L448-L536】
  - `SlotAdmin::spin` then calls `SlotMath::post_process` and persists actions via repositories.【F:fugaso_core/src/admin.rs†L536-L610】
- **Respin flow**
  - Dispatcher `on_respin` calls `SlotAdmin::respin`, which invokes `SlotMath::respin` and then `post_process` to update `GameData` and actions.【F:fugaso_admin/src/dispatcher.rs†L894-L912】【F:fugaso_core/src/admin.rs†L563-L610】
- **Free spin and collect flow**
  - Free spin uses `SlotMath::free_spin`; collect uses `SlotMath::collect` with a `SpinArg` (stake is 0 for collect).【F:fugaso_core/src/admin.rs†L614-L742】
- **Game math specifics**
  - `fugaso_math_ed6` and `fugaso_math_ed7` implement the math logic and set `GameData::Spin` vs `GameData::ReSpin` based on `ActionKind` from computed results; see `post_process` and `spin`/`respin` handling in those modules.【F:fugaso_math_ed6/src/math.rs†L452-L615】【F:fugaso_math_ed7/src/math.rs†L456-L619】

# Coding Rules
- Keep `SlotMath` implementations pure to math/state transitions; persistence and networking belong in `fugaso_core`/`fugaso_admin` layers (see `SlotAdmin` and dispatcher).【F:fugaso_core/src/admin.rs†L290-L742】【F:fugaso_admin/src/dispatcher.rs†L700-L940】
- Use `fugaso_action::ActionKind`/`Model` when representing stored actions and state transitions; do not invent new action values outside `ActionKind` enum.【F:fugaso_data/src/fugaso_action.rs†L18-L71】
- When adding new protocol fields, update both `GameResult`/`SpinData` and related serialization logic to keep the public API consistent.【F:fugaso_math/src/protocol.rs†L260-L403】

# Search & Debugging Guidelines
- **Find all `SlotMath` implementations:** search for `impl.*SlotMath` (e.g., `impl<R: ThunderExpressRand> SlotMath` or `impl<R: MegaThunderRand> SlotMath`).【F:fugaso_math_ed6/src/math.rs†L349-L370】【F:fugaso_math_ed7/src/math.rs†L356-L378】
- **Find call sites of `join`/`init`/`spin`/`respin`:** search for `\.join\(`, `\.init\(`, `\.spin\(`, `\.respin\(` in `fugaso_core/src/admin.rs` and `fugaso_admin/src/dispatcher.rs`.【F:fugaso_core/src/admin.rs†L290-L610】【F:fugaso_admin/src/dispatcher.rs†L700-L940】
- **Trace action/state transitions:** search `ActionKind::RESPIN`, `ActionKind::FREE_SPIN`, `ActionKind::COLLECT`, and `fsm.server_act`/`fsm.client_act` in `fugaso_core/src/admin.rs` and `fugaso_math/src/fsm.rs`.【F:fugaso_core/src/admin.rs†L448-L746】【F:fugaso_math/src/fsm.rs†L26-L160】
- **Locate `GameData` creation:** search for `GameData::Spin`, `GameData::ReSpin`, `GameData::Initial`, `GameData::Collect`, `GameData::FreeSpin` in math implementations and protocol helpers.【F:fugaso_math/src/protocol.rs†L24-L53】【F:fugaso_math_ed6/src/math.rs†L452-L615】【F:fugaso_math_ed7/src/math.rs†L456-L619】
- **Trace stored actions:** search `fugaso_action::Model` usages in `fugaso_core::admin` and `fugaso_math::protocol` conversions (`GameResult::from_action` / `to_action`).【F:fugaso_core/src/admin.rs†L19-L76】【F:fugaso_math/src/protocol.rs†L392-L451】

# What NOT to do
- Do **not** guess how external crates (`essential_*`, `sea-orm`) behave; treat them as black boxes unless the behavior is visible in this repository.【F:Cargo.toml†L16-L35】
- Do **not** assume where `JoinArg`, `GameInitArg`, or `SpinArg` values originate; follow the actual construction in `SlotAdmin` and dispatcher code paths.【F:fugaso_core/src/admin.rs†L290-L610】【F:fugaso_admin/src/dispatcher.rs†L700-L940】
- Do **not** infer missing game rules; math logic is specific to each game module (`fugaso_math_ed6`, `fugaso_math_ed7`). Use those implementations directly for behavior details.【F:fugaso_math_ed6/src/math.rs†L17-L120】【F:fugaso_math_ed7/src/math.rs†L17-L120】
- Do **not** conflate `ReSpin` vs `FreeSpin` vs `Collect`; `GameData` variants and `ActionKind` values must match actual transitions defined in the math/FSM code.【F:fugaso_math/src/protocol.rs†L24-L53】【F:fugaso_math/src/fsm.rs†L26-L160】
