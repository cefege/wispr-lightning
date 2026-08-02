#!/usr/bin/env bash
#
# End-to-end harness: drives the REAL Wispr Lightning application.
#
# Everything here runs a shipped artifact — the signed .app bundle, or a
# `cargo build` binary where the row needs a rebuild — and asserts through the
# macOS accessibility API, CoreGraphics' window list, the application's own
# log, the keychain and the filesystem. Nothing is mocked and no application
# source is modified.
#
# Rows: LIF-006, LIF-009, LIF-012, LIF-014, LIF-018, LIF-021,
#       AUT-001, AUT-003, AUT-005, AUT-016, AUT-032, PRV-025.
#
# Why a script rather than `src-tauri/tests/app_e2e.rs`: see scripts/README.md.
#
# Usage:
#   scripts/e2e.sh                 # every row
#   scripts/e2e.sh LIF-006 AUT-005 # named rows only
#
# Safety contract, enforced by `baseline` and `verify_untouched`:
#   * the user's real ~/Library/Application Support/Wispr Flow/session.json is
#     checksummed before and after and is never opened for writing;
#   * the user's real keyring item is checksummed before and after; every row
#     needing a keyring runs against a throwaway keychain in a throwaway HOME,
#     so the app exercises real Keychain Services in isolation;
#   * src-tauri/capabilities/*.json are checksummed before and after the
#     LIF-021 mutation and restored from a byte copy, including on abort;
#   * only processes this script started are ever signalled. The user's own
#     /Applications/Wispr Lightning.app is left strictly alone.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

export PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/agent-AppE2e}"

BUNDLE_APP="${WL_BUNDLE:-$ROOT/target/main/release/bundle/macos/Wispr Lightning.app}"
BUNDLE_BIN="$BUNDLE_APP/Contents/MacOS/wispr-lightning"
BARE_BIN="$CARGO_TARGET_DIR/debug/wispr-lightning"

# `pwd -P` matters: /tmp is a symlink to /private/tmp, and Tauri refuses to
# resolve `current_exe()` through a symlink (tauri-utils' starting_binary),
# so an app bundle copied under the /tmp spelling dies at startup with
# "unknown path".
RUN="$(cd "$(mktemp -d /tmp/wl-e2e.XXXXXX)" && pwd -P)"
BIN="$RUN/bin"
SRC="$ROOT/scripts/e2e"
mkdir -p "$BIN"

FLOW_SESSION="$HOME/Library/Application Support/Wispr Flow/session.json"
CAPS=(src-tauri/capabilities/default.json src-tauri/capabilities/overlay.json)

STARTED_PIDS=()
RESULTS=()
FAILURES=0
CAFFEINATE_PID=""

say()   { printf '%s\n' "$*"; }
head1() { printf '\n=== %s ===\n' "$*"; }
head2() { printf -- '--- %s\n' "$*"; }

pass() { RESULTS+=("CLOSED     $1 — $2"); printf 'CLOSED     %s — %s\n' "$1" "$2"; }
fail() {
  RESULTS+=("NOT CLOSED $1 — $2")
  printf 'NOT CLOSED %s — %s\n' "$1" "$2"
  FAILURES=$((FAILURES + 1))
}

# ---------------------------------------------------------------------------
# Process control
#
# Never `open -a`, never match by process NAME. This machine also hosts
# /Applications/Wispr Lightning.app, which carries the SAME bundle identifier
# and the SAME two URL schemes (MATRIX LIF-023, AUT-004), so both LaunchServices
# resolution and name lookup are ambiguous. Everything here is launched by
# absolute path and tracked by the pid we were handed.
# ---------------------------------------------------------------------------

start_app() { # label binary home cwd [VAR=value ...]
  local label="$1" bin="$2" home="$3" cwd="$4"
  shift 4
  rm -f "$RUN/$label.pid" "$RUN/$label.exit"
  (
    cd "$cwd" || exit 1
    env HOME="$home" "$@" "$bin" >"$RUN/$label.out" 2>"$RUN/$label.err" &
    child=$!
    echo "$child" >"$RUN/$label.pid"
    wait "$child"
    echo "$?" >"$RUN/$label.exit"
    # The supervising subshell must not inherit this function's stdout: a
    # command substitution around `start_app` waits for every writer to close
    # the pipe, and this one stays open for the whole life of the app.
  ) >/dev/null 2>&1 &
  local waited=0
  while [ ! -s "$RUN/$label.pid" ] && [ "$waited" -lt 50 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  local pid
  pid="$(cat "$RUN/$label.pid" 2>/dev/null)"
  [ -n "$pid" ] || return 1
  STARTED_PIDS+=("$pid")
  echo "$pid"
}

app_alive() { kill -0 "$1" 2>/dev/null; }

stop_app() {
  local pid="$1" waited=0
  app_alive "$pid" || return 0
  kill -TERM "$pid" 2>/dev/null
  while app_alive "$pid" && [ "$waited" -lt 50 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  app_alive "$pid" && kill -KILL "$pid" 2>/dev/null
  return 0
}

wait_log() { # file pattern seconds
  local file="$1" pattern="$2" limit="${3:-15}" waited=0
  while [ "$waited" -lt $((limit * 10)) ]; do
    grep -q -- "$pattern" "$file" 2>/dev/null && return 0
    sleep 0.1
    waited=$((waited + 1))
  done
  return 1
}

# The permission sweep is the last thing `setup()` logs, so it is the cheapest
# honest readiness signal the app already emits.
wait_ready() { wait_log "$RUN/$1.err" 'permission' "${2:-30}"; }

# ---------------------------------------------------------------------------
# Observation
# ---------------------------------------------------------------------------

windows_of() { "$BIN/wlwindows" "$1"; }

window_rect() { # pid -> "x y w h" of the first on-screen non-overlay window
  windows_of "$1" | grep 'onscreen=true' | grep -v 'w=120' | head -1 |
    sed 's/.*x=\([0-9-]*\).y=\([0-9-]*\).w=\([0-9]*\).h=\([0-9]*\).*/\1 \2 \3 \4/'
}

se() { # pid applescript-body — addressed by pid, never by name
  local pid="$1"
  shift
  osascript <<APPLESCRIPT 2>&1
tell application "System Events"
  tell (first process whose unix id is $pid)
$*
  end tell
end tell
APPLESCRIPT
}

tray_items() { se "$1" 'return (count of menu bar items of menu bar 2)'; }

tray_click() { # pid item-name
  se "$1" "
    click menu bar item 1 of menu bar 2
    delay 0.7
    click menu item \"$2\" of menu 1 of menu bar item 1 of menu bar 2"
}

# WebKit does not vend the web view's accessibility tree until the owning app
# has been activated at least once. This app is an accessory that does not
# activate itself (MATRIX SET-009), so every UI read starts here.
activate() {
  se "$1" 'set frontmost to true' >/dev/null
  sleep 1.2
}

ax()  { "$BIN/wlax" "$@"; }

shot() { # pid path — screencapture -l is refused for these windows, so region
  local rect
  rect="$(window_rect "$1")"
  [ -n "$rect" ] || return 1
  # shellcheck disable=SC2086
  set -- "$1" "$2" $rect
  screencapture -x -o -R "$3,$4,$5,$6" "$2" 2>/dev/null && [ -s "$2" ]
}

sha() { shasum -a 256 "$1" 2>/dev/null | awk '{print $1}'; }

# ---------------------------------------------------------------------------
# Throwaway HOME with a REAL but isolated login keychain.
#
# Security framework resolves the default keychain through $HOME, which is what
# makes this safe: the app under test runs the genuine keyring code path
# (CredentialStore -> keyring -> Keychain Services) while the user's own login
# keychain is never opened. Verified rather than assumed — a temp HOME with no
# keychain makes the app log "OS keyring unavailable", and the AUT-016 row
# asserts that warning is ABSENT.
#
# It also isolates the database, settings, logs, spool and — once the Wispr
# Flow watcher lands — the directory that watcher observes.
# ---------------------------------------------------------------------------

make_home() {
  local home="$RUN/home-$1"
  mkdir -p "$home/Library/Keychains"
  HOME="$home" security create-keychain -p wl-e2e "$home/Library/Keychains/login.keychain-db" >/dev/null 2>&1
  HOME="$home" security default-keychain -s "$home/Library/Keychains/login.keychain-db" >/dev/null 2>&1
  HOME="$home" security unlock-keychain -p wl-e2e "$home/Library/Keychains/login.keychain-db" >/dev/null 2>&1
  echo "$home"
}

# ---------------------------------------------------------------------------
# Safety baseline
# ---------------------------------------------------------------------------

baseline() {
  head1 "Baseline — system state that MUST come back unchanged"
  BASE_FLOW="$(sha "$FLOW_SESSION")"
  say "Wispr Flow session.json   $BASE_FLOW  ($(stat -f %z "$FLOW_SESSION" 2>/dev/null) bytes)"
  BASE_CAPS=()
  local cap
  for cap in "${CAPS[@]}"; do
    BASE_CAPS+=("$(sha "$cap")")
    say "$cap  $(sha "$cap")"
  done
  BASE_SESSION_BLOB="$(security find-generic-password -s com.wisprlightning.app -a wispr-session -w 2>/dev/null |
    shasum -a 256 | awk '{print $1}')"
  say "keyring wispr-session     ${BASE_SESSION_BLOB:-<absent>}"
}

verify_untouched() {
  head1 "Post-run verification — system state"
  local bad=0 now i
  now="$(sha "$FLOW_SESSION")"
  if [ "$now" = "$BASE_FLOW" ]; then
    say "OK   Wispr Flow session.json unchanged   $now"
  else
    say "FAIL Wispr Flow session.json CHANGED     $BASE_FLOW -> $now"
    bad=1
  fi
  now="$(security find-generic-password -s com.wisprlightning.app -a wispr-session -w 2>/dev/null |
    shasum -a 256 | awk '{print $1}')"
  if [ "${now:-<absent>}" = "${BASE_SESSION_BLOB:-<absent>}" ]; then
    say "OK   keyring wispr-session unchanged     ${now:-<absent>}"
  else
    say "FAIL keyring wispr-session CHANGED       ${BASE_SESSION_BLOB:-<absent>} -> ${now:-<absent>}"
    bad=1
  fi
  for i in "${!CAPS[@]}"; do
    now="$(sha "${CAPS[$i]}")"
    if [ "$now" = "${BASE_CAPS[$i]}" ]; then
      say "OK   ${CAPS[$i]} byte-identical  $now"
    else
      say "FAIL ${CAPS[$i]} CHANGED  ${BASE_CAPS[$i]} -> $now"
      bad=1
    fi
  done
  local stray
  # A SIGTERMed app takes a moment to unwind AppKit; give it one before
  # calling a process that is already on its way out a leak.
  sleep 2
  stray="$(pgrep -f "$ROOT/target|$RUN" 2>/dev/null | tr '\n' ' ')"
  if [ -z "$stray" ]; then
    say "OK   no stray processes under $ROOT/target"
  else
    say "FAIL stray processes: $stray"
    bad=1
  fi
  say "NOTE /Applications/Wispr Lightning.app (the user's own app) was never signalled."
  return $bad
}

cleanup() {
  local pid cap
  for pid in "${STARTED_PIDS[@]:-}"; do
    [ -n "$pid" ] && stop_app "$pid"
  done
  for cap in "${CAPS[@]}"; do
    if [ -f "$RUN/$(basename "$cap").orig" ]; then
      cp "$RUN/$(basename "$cap").orig" "$cap"
      say "restored $cap from the pre-mutation copy"
    fi
  done
  [ -n "$CAFFEINATE_PID" ] && kill "$CAFFEINATE_PID" 2>/dev/null
  return 0
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

build_helpers() {
  head2 "compiling the window / accessibility / Apple Event / opener helpers"
  swiftc -O -o "$BIN/wlwindows" "$SRC/wlwindows.swift" || return 1
  swiftc -O -o "$BIN/wlax" "$SRC/wlax.swift" || return 1
  swiftc -O -o "$BIN/wlopenurl" "$SRC/wlopenurl.swift" || return 1
  clang -dynamiclib -o "$BIN/opener-shim.dylib" "$SRC/opener_shim.c" || return 1
  say "helpers built into $BIN"
  ax trusted || return 1
}

build_app() {
  head2 "cargo build -p wispr-lightning"
  cargo build -p wispr-lightning 2>&1 | tail -4
  [ -x "$BARE_BIN" ] || return 1
  # `tauri build` copies these next to the executable; a plain `cargo build`
  # does not, and where the app looks for them is exactly LIF-018's claim.
  mkdir -p "$(dirname "$BARE_BIN")/resources"
  cp -R "$ROOT/src-tauri/resources/sounds" "$(dirname "$BARE_BIN")/resources/"
}

# ---------------------------------------------------------------------------
# LIF-006 — the overlay window exists at launch but is not visible.
# ---------------------------------------------------------------------------

row_LIF_006() {
  head1 "LIF-006  overlay constructed at launch, not shown"
  local home pid before after overlay overlay_after visible_count
  home="$(make_home lif006)"
  pid="$(start_app lif006 "$BUNDLE_BIN" "$home" "$ROOT")" || { fail LIF-006 "could not launch"; return; }
  wait_ready lif006 || { fail LIF-006 "app never finished setup"; return; }
  sleep 1

  head2 "CGWindowList .optionAll for pid $pid (includes off-screen windows)"
  before="$(windows_of "$pid")"
  say "$before"
  # 120x36 is overlay.rs INITIAL_WIDTH x OVERLAY_HEIGHT; layer 3 is the
  # floating NSPanel level the overlay is hardened to.
  overlay="$(echo "$before" | grep -E 'w=120[[:space:]]+h=36')"

  head2 "open the Settings window, so 'nothing is visible' cannot be the reason"
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  after="$(windows_of "$pid")"
  say "$after"
  visible_count="$(echo "$after" | grep -c 'onscreen=true')"
  overlay_after="$(echo "$after" | grep -E 'w=120[[:space:]]+h=36')"
  stop_app "$pid"

  if [ -z "$overlay" ]; then
    fail LIF-006 "no 120x36 window at launch — the overlay was not constructed"
  elif ! echo "$overlay" | grep -q 'onscreen=false'; then
    fail LIF-006 "the overlay is on screen at launch: $overlay"
  elif [ "$visible_count" -lt 1 ]; then
    fail LIF-006 "no window ever became visible, so 'not visible' proves nothing"
  elif ! echo "$overlay_after" | grep -q 'onscreen=false'; then
    fail LIF-006 "the overlay became visible without a recording: $overlay_after"
  else
    pass LIF-006 "overlay present at 120x36 layer 3 with onscreen=false at launch, and still off screen once the Settings window is on screen"
  fi
}

# ---------------------------------------------------------------------------
# LIF-009 — the deep-link handler is registered at launch.
# ---------------------------------------------------------------------------

row_LIF_009() {
  head1 "LIF-009  deep-link handler registered at launch"
  head2 "static registration in the bundle"
  /usr/libexec/PlistBuddy -c 'Print :CFBundleURLTypes' "$BUNDLE_APP/Contents/Info.plist"

  local home pid got_lightning=1 got_flow=1
  home="$(make_home lif009)"
  pid="$(start_app lif009 "$BUNDLE_BIN" "$home" "$ROOT")" || { fail LIF-009 "could not launch"; return; }
  wait_ready lif009 || { fail LIF-009 "app never finished setup"; return; }

  head2 "deliver a GURL Apple Event — the exact event LaunchServices sends — to pid $pid"
  # An auth link carrying parameters but no tokens: it reaches the handler and
  # is rejected loudly, so delivery is provable without writing any session.
  "$BIN/wlopenurl" "$pid" 'wisprlightning://auth/google/success?probe=lif009'
  wait_log "$RUN/lif009.err" 'auth callback carried no usable tokens' 10 && got_lightning=0
  "$BIN/wlopenurl" "$pid" 'wispr-flow://auth/google/success?probe=lif009flow'
  sleep 1.5
  [ "$(grep -c 'auth callback carried no usable tokens' "$RUN/lif009.err")" -ge 2 ] && got_flow=0

  head2 "app log"
  grep 'deeplink' "$RUN/lif009.err" || say "(nothing)"
  stop_app "$pid"

  if [ $got_lightning -eq 0 ] && [ $got_flow -eq 0 ]; then
    pass LIF-009 "both declared schemes reached the handler at runtime; Info.plist declares wispr-flow and wisprlightning"
  elif [ $got_lightning -eq 0 ]; then
    fail LIF-009 "wisprlightning:// reached the handler, wispr-flow:// did not"
  else
    fail LIF-009 "no GURL event reached the handler"
  fi
}

# ---------------------------------------------------------------------------
# LIF-012 — clean shutdown on a tray Quit.
# ---------------------------------------------------------------------------

row_LIF_012() {
  head1 "LIF-012  tray Quit closes the database handle cleanly"
  local home pid db code integrity wal_running wal_after waited=0
  home="$(make_home lif012)"
  db="$home/Library/Application Support/WisprLightning/lightning.db"

  pid="$(start_app lif012 "$BUNDLE_BIN" "$home" "$ROOT")" || { fail LIF-012 "could not launch"; return; }
  wait_ready lif012 || { fail LIF-012 "app never finished setup"; return; }
  sleep 1

  head2 "database files while the app is running"
  ls -la "$(dirname "$db")" | grep lightning
  wal_running="$(stat -f %z "$db-wal" 2>/dev/null || echo 0)"

  head2 "click Quit Wispr Lightning in the tray menu"
  tray_click "$pid" "Quit Wispr Lightning" >/dev/null
  while app_alive "$pid" && [ "$waited" -lt 150 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  code="$(cat "$RUN/lif012.exit" 2>/dev/null)"

  head2 "exit status and shutdown log"
  say "exit code: ${code:-<still running>}"
  tail -3 "$RUN/lif012.err"

  head2 "database files after quit"
  ls -la "$(dirname "$db")" | grep lightning
  wal_after="$(stat -f %z "$db-wal" 2>/dev/null || echo 0)"
  say "write-ahead log: $wal_running bytes while running -> $wal_after bytes after quit"
  say "(closing the last connection checkpoints the WAL and truncates it to 0."
  say " Presence of the -wal and -shm FILES is not a signal on macOS — Apple's"
  say " SQLite leaves both behind even after a clean close, verified against"
  say " the sqlite3 CLI on a scratch database. The SIZE is the signal.)"
  # Runs last: opening the database to check it would itself checkpoint the
  # WAL and destroy the measurement above.
  integrity="$(sqlite3 "$db" 'PRAGMA integrity_check;' 2>&1)"
  say "PRAGMA integrity_check: $integrity"

  if app_alive "$pid"; then
    fail LIF-012 "the app survived a tray Quit"
  elif [ "$code" != "0" ]; then
    fail LIF-012 "tray Quit exited $code, not 0"
  elif ! grep -q 'Wispr Lightning: shutting down' "$RUN/lif012.err"; then
    fail LIF-012 "no shutdown line — the process ended without running RunEvent::Exit"
  elif [ "$integrity" != "ok" ]; then
    fail LIF-012 "integrity_check returned: $integrity"
  elif [ "$wal_after" != "0" ]; then
    fail LIF-012 "tray Quit exits 0, logs the shutdown line and leaves the database uncorrupted (integrity_check ok) — but the handle is NOT closed: the WAL is still $wal_after bytes un-checkpointed. tauri::App::run never returns; it ends the process with std::process::exit (tauri-2.11.5/src/app.rs:578), so managed state is never dropped and rusqlite's Connection is never closed."
  else
    pass LIF-012 "tray Quit logs the shutdown line and exits 0; the WAL is checkpointed to 0 bytes and integrity_check is ok, so the handle was closed rather than abandoned"
  fi
}

# ---------------------------------------------------------------------------
# LIF-014 — single instance.
# ---------------------------------------------------------------------------

row_LIF_014() {
  head1 "LIF-014  a second launch exits instead of starting a second tray icon"
  head2 "same-identifier third-party app present on this machine?"
  local foreign
  foreign="$(pgrep -f '/Applications/Wispr Lightning.app' | tr '\n' ' ')"
  if [ -n "$foreign" ]; then
    say "YES — /Applications/Wispr Lightning.app is running as pid(s) $foreign"
    say "     identifier $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' '/Applications/Wispr Lightning.app/Contents/Info.plist' 2>/dev/null)"
    say "     The guard is therefore exercised by absolute path, never through"
    say "     LaunchServices. Its rendezvous is /tmp/com_wisprlightning_app_si.sock,"
    say "     which the Swift app does not use — so the guard is measurable, but"
    say "     LaunchServices arbitration on this machine genuinely is ambiguous."
  else
    say "no"
  fi

  local home pid1 pid2 items_before items_after code2 waited=0
  home="$(make_home lif014)"
  pid1="$(start_app lif014a "$BUNDLE_BIN" "$home" "$ROOT")" || { fail LIF-014 "could not launch the first instance"; return; }
  wait_ready lif014a || { fail LIF-014 "first instance never finished setup"; return; }
  sleep 1
  items_before="$(tray_items "$pid1")"
  head2 "first instance pid $pid1 — menu bar 2 items: $items_before"

  head2 "launch the same binary a second time"
  pid2="$(start_app lif014b "$BUNDLE_BIN" "$home" "$ROOT")" || { fail LIF-014 "could not launch the second instance"; return; }
  say "second instance pid $pid2"
  while app_alive "$pid2" && [ "$waited" -lt 150 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
  code2="$(cat "$RUN/lif014b.exit" 2>/dev/null)"
  say "second instance exit code: ${code2:-<still running>}"
  head2 "second instance stderr in full"
  cat "$RUN/lif014b.err"
  head2 "did the first instance observe the second launch?"
  grep 'second instance' "$RUN/lif014a.err" || say "(no line)"

  sleep 1
  items_after="$(tray_items "$pid1")"
  head2 "first instance alive: $(app_alive "$pid1" && echo yes || echo no) — menu bar 2 items: $items_after"
  head2 "hotkey hooks installed"
  say "first:  $(grep -c 'global hotkey listener active' "$RUN/lif014a.err")"
  say "second: $(grep -c 'global hotkey listener active' "$RUN/lif014b.err")"
  stop_app "$pid1"

  if [ -z "$code2" ]; then
    fail LIF-014 "the second instance is still running"
  elif [ "$code2" != "0" ]; then
    fail LIF-014 "the second instance exited $code2, not 0"
  elif [ "$items_after" != "1" ] || [ "$items_before" != "1" ]; then
    fail LIF-014 "tray item count is $items_before -> $items_after, expected 1 -> 1"
  elif [ "$(grep -c 'global hotkey listener active' "$RUN/lif014b.err")" -ne 0 ]; then
    fail LIF-014 "the second instance installed a hotkey hook before exiting"
  elif ! grep -q 'second instance launched' "$RUN/lif014a.err"; then
    fail LIF-014 "the first instance never observed the second launch"
  else
    pass LIF-014 "second launch exits 0 with no tray icon and no hotkey hook; the first instance survives with exactly one menu bar item and logs the hand-off"
  fi
}

# ---------------------------------------------------------------------------
# LIF-018 — bundled resources resolve through the resource mechanism.
# ---------------------------------------------------------------------------

row_LIF_018() {
  head1 "LIF-018  resources resolve relative to the executable, not the cwd"
  local home pid packs strip warned_b=1 warned_c=0

  head2 "A. the signed bundle, launched with cwd=/ — nothing is relative to /"
  home="$(make_home lif018a)"
  pid="$(start_app lif018a "$BUNDLE_BIN" "$home" /)" || { fail LIF-018 "could not launch"; return; }
  wait_ready lif018a || { fail LIF-018 "app never finished setup"; return; }
  sleep 1
  if grep -q 'no sounds directory' "$RUN/lif018a.err"; then
    grep 'sounds' "$RUN/lif018a.err"
    stop_app "$pid"
    fail LIF-018 "running from / broke resource resolution"
    return
  fi
  say "the 'no sounds directory in the bundle' warning is absent — the resource root resolved"
  say "bundle resource tree: $(find "$BUNDLE_APP/Contents/Resources/resources/sounds" -maxdepth 1 -type d -mindepth 1 | wc -l | tr -d ' ') packs"

  head2 "the running app's own Sound pack picker (best effort — the row does not rest on it)"
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  activate "$pid"
  ax press "$pid" "System" >/dev/null 2>&1
  sleep 1.5
  packs="$(ax pane "$pid" 2>&1 | grep -iE '^(default|v1|v2|v3)$' | tr '\n' ' ')"
  say "picker values visible: ${packs:-<none read>}"
  shot "$pid" "$RUN/lif018-system-pane.png" && say "screenshot: $RUN/lif018-system-pane.png"
  stop_app "$pid"

  head2 "B. the same binary alone in an empty directory, cwd = the repository root"
  say "    the repo root contains Resources/Sounds AND src-tauri/resources/sounds,"
  say "    and the original bundle still sits at its own path, so a cwd-relative"
  say "    lookup or any hardcoded location would still succeed here"
  strip="$RUN/relocated"
  mkdir -p "$strip"
  cp "$BUNDLE_BIN" "$strip/wispr-lightning"
  home="$(make_home lif018b)"
  pid="$(start_app lif018b "$strip/wispr-lightning" "$home" "$ROOT")" || { fail LIF-018 "could not launch the relocated copy"; return; }
  wait_ready lif018b 30
  sleep 1
  grep 'sounds' "$RUN/lif018b.err" || say "(no sounds line at all)"
  grep -q 'no sounds directory' "$RUN/lif018b.err" && warned_b=0
  stop_app "$pid"

  head2 "C. the SAME relocated binary, with the resources copied next to it, cwd=/"
  cp -R "$ROOT/src-tauri/resources" "$strip/resources"
  home="$(make_home lif018c)"
  pid="$(start_app lif018c "$strip/wispr-lightning" "$home" /)" || { fail LIF-018 "could not launch case C"; return; }
  wait_ready lif018c 30
  sleep 1
  grep 'sounds' "$RUN/lif018c.err" || say "no sounds warning — resolved from beside the executable"
  grep -q 'no sounds directory' "$RUN/lif018c.err" && warned_c=1
  stop_app "$pid"

  if [ $warned_b -ne 0 ]; then
    fail LIF-018 "a copy of the binary with no resources beside it still resolved sounds — the lookup is not executable-relative"
  elif [ $warned_c -ne 0 ]; then
    fail LIF-018 "the relocated binary could not resolve resources placed next to it"
  else
    pass LIF-018 "three cases pin the mechanism: the bundle resolves its sounds with cwd=/ (A); the identical binary alone in an empty directory resolves nothing even with the repository — which holds two copies of the sound packs — as cwd (B); and the same binary in the same directory resolves them again as soon as the resources are placed beside the executable, still with cwd=/ (C). The path derives from the executable, not from the working directory and not from any fixed location."
  fi
}

# ---------------------------------------------------------------------------
# LIF-021 — an undeclared capability is refused at runtime.
# ---------------------------------------------------------------------------

row_LIF_021() {
  head1 "LIF-021  removing a permission makes the command fail at runtime"
  local caps=src-tauri/capabilities/default.json with_panel without_panel
  cp "$caps" "$RUN/default.json.orig"
  head2 "checksum before"
  shasum -a 256 "$caps"

  head2 "A. baseline build, dialog:allow-open declared — Import CSV must open a panel"
  build_app || { fail LIF-021 "baseline build failed"; return; }
  with_panel="$(csv_import_opens_panel lif021a)"
  say "native open panel appeared: $with_panel"

  head2 "B. remove exactly one permission — dialog:allow-open — and rebuild"
  python3 - "$caps" <<'PY'
import json, sys
path = sys.argv[1]
with open(path) as fh:
    doc = json.load(fh)
doc["permissions"] = [p for p in doc["permissions"] if p != "dialog:allow-open"]
with open(path, "w") as fh:
    json.dump(doc, fh, indent=2)
    fh.write("\n")
PY
  grep -n 'dialog' "$caps" || say "(no dialog permission left in the file)"
  if ! build_app; then
    cp "$RUN/default.json.orig" "$caps"
    rm -f "$RUN/default.json.orig"
    fail LIF-021 "the mutated build failed to compile"
    return
  fi
  without_panel="$(csv_import_opens_panel lif021b)"
  say "native open panel appeared: $without_panel"
  head2 "what each build logged about it"
  say "baseline: $(grep -ciE 'dialog|denied|not allowed|capabilit' "$RUN/lif021a.err") lines mentioning a permission"
  say "mutated:  $(grep -ciE 'dialog|denied|not allowed|capabilit' "$RUN/lif021b.err") lines mentioning a permission"
  say "(the refusal is returned to the webview, not logged by the Rust side —"
  say " which is precisely the silent failure this row exists to characterise)"

  head2 "C. restore and re-verify"
  cp "$RUN/default.json.orig" "$caps"
  rm -f "$RUN/default.json.orig"
  shasum -a 256 "$caps"

  if [ "$with_panel" != "yes" ]; then
    fail LIF-021 "the baseline build did not open a panel either, so the negative result proves nothing"
  elif [ "$without_panel" != "no" ]; then
    fail LIF-021 "the command still worked with its permission removed — the capability guard did NOT bite"
  else
    pass LIF-021 "with dialog:allow-open declared, Import CSV opens a native open panel; with that one permission removed and nothing else changed, the same click opens nothing — the command is refused at runtime and nothing is logged"
  fi
}

# Drive Settings -> Dictionary -> Snippets -> Import CSV and report whether a
# native open panel appeared. The panel is an AXSheet on the settings window.
csv_import_opens_panel() { # label -> yes | no | error
  local label="$1" home pid sheets answer
  home="$(make_home "$label")"
  pid="$(start_app "$label" "$BARE_BIN" "$home" "$ROOT")" || { echo error; return; }
  wait_ready "$label" 30 || { echo error; return; }
  sleep 1
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  activate "$pid"
  ax press "$pid" "Dictionary" >/dev/null 2>&1
  sleep 1.2
  ax press "$pid" "Snippets" >/dev/null 2>&1
  sleep 1
  ax press "$pid" "Import CSV" >/dev/null 2>&1
  sleep 3
  sheets="$(ax sheets "$pid" 2>&1)"
  echo "$sheets" >"$RUN/$label.sheets"
  shot "$pid" "$RUN/$label.png"
  if echo "$sheets" | grep -q 'sheets=[1-9]'; then
    answer=yes
  else
    answer=no
  fi
  # Dismiss any panel before the app is torn down.
  se "$pid" 'key code 53' >/dev/null 2>&1
  sleep 0.5
  stop_app "$pid"
  echo "$answer"
}

# ---------------------------------------------------------------------------
# AUT-001 / AUT-003 — sign-in hands the system opener the exact authorize URL.
# ---------------------------------------------------------------------------

row_AUT_001() {
  head1 "AUT-001 / AUT-003  sign-in opens the system browser at the authorize URL"
  local home pid url log="$RUN/opener.log"
  local expected='https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/authorize?provider=google&redirect_to=wispr-flow://auth/google/success'
  : >"$log"
  home="$(make_home aut001)"
  # The shim interposes posix_spawn: it records what the app asked the OS to
  # open and substitutes /usr/bin/true for /usr/bin/open, so no browser window
  # is raised and no real sign-in can begin.
  #
  # It cannot be loaded into the signed bundle: hardened runtime
  # (flags=0x10000) enforces library validation and the bundle carries no
  # `com.apple.security.cs.disable-library-validation`, so dyld drops
  # DYLD_INSERT_LIBRARIES silently — verified twice, once with an unsigned
  # shim and once with one signed by the same "Claude Voice Dev" identity
  # (which has no Team ID, so it cannot satisfy validation either): the button
  # press succeeds and the log stays empty.
  #
  # The unbundled `cargo build` binary loads it but is useless here for the
  # opposite reason: with no bundle identifier macOS refuses to activate the
  # process (MATRIX SET-009), and WebKit does not vend the web view's
  # accessibility tree to an app that has never been active, so the Sign In
  # button cannot be found, let alone pressed.
  #
  # So the row runs a copy of the real bundle, re-signed ad hoc without
  # hardened runtime. Two deliberate edits keep it out of the way of the
  # machine's other two same-identity apps: a distinct CFBundleIdentifier so
  # LaunchServices never sees a third `com.wisprlightning.app`, and no
  # CFBundleURLTypes at all so it can never win a deep-link race. Neither
  # touches the code under test — `auth_sign_in` builds the URL from a
  # compiled-in constant and hands it to `tauri_plugin_opener`.
  local probe="$RUN/AuthProbe.app"
  cp -R "$BUNDLE_APP" "$probe"
  /usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier com.wisprlightning.e2e-authprobe" "$probe/Contents/Info.plist" >/dev/null
  /usr/libexec/PlistBuddy -c "Delete :CFBundleURLTypes" "$probe/Contents/Info.plist" >/dev/null 2>&1
  codesign --remove-signature "$probe" >/dev/null 2>&1
  codesign -f -s - "$probe" >/dev/null 2>&1
  head2 "the probe bundle's signature and identity"
  codesign -dv "$probe" 2>&1 | grep -iE 'Identifier=|flags='

  pid="$(start_app aut001 "$probe/Contents/MacOS/wispr-lightning" "$home" "$ROOT" \
    DYLD_INSERT_LIBRARIES="$BIN/opener-shim.dylib" WL_OPEN_LOG="$log")" ||
    { fail AUT-001 "could not launch"; return; }
  wait_ready aut001 30 || { fail AUT-001 "app never finished setup"; return; }
  sleep 1
  head2 "the shim is actually loaded into pid $pid"
  say "opener-shim mappings: $(vmmap "$pid" 2>/dev/null | grep -ci opener-shim)"

  head2 "click Sign In with Google in the running Settings window"
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  activate "$pid"
  ax press "$pid" "Sign In with Google"
  sleep 2.5
  stop_app "$pid"

  head2 "every process the app asked the OS to spawn"
  cat "$log"
  url="$(grep -o 'https://[^[:space:]]*' "$log" | head -1)"
  say "observed: ${url:-<nothing>}"
  say "expected: $expected"

  if [ -z "$url" ]; then
    fail AUT-001 "the app never asked the OS to open anything"
  elif [ "$url" != "$expected" ]; then
    fail AUT-001 "URL mismatch"
  elif ! grep -q '/usr/bin/open' "$log"; then
    fail AUT-003 "the URL did not go through the system opener"
  else
    pass AUT-001 "the app handed the OS opener exactly $expected"
    pass AUT-003 "the target is /usr/bin/open — the OS default-handler entry point — and no window in this process navigated anywhere"
  fi
}

# ---------------------------------------------------------------------------
# AUT-005 / AUT-016 — the OAuth callback arrives by deep link and is saved.
# ---------------------------------------------------------------------------

row_AUT_005() {
  head1 "AUT-005 / AUT-016  callback through the deep-link handler saves the session"
  local home pid url kc seeded before after before_items after_items signed=1
  home="$(make_home aut005)"
  pid="$(start_app aut005 "$BUNDLE_BIN" "$home" "$ROOT")" || { fail AUT-005 "could not launch"; return; }
  wait_ready aut005 30 || { fail AUT-005 "app never finished setup"; return; }
  sleep 1

  head2 "is the app talking to a REAL keychain? (the warning below must be absent)"
  grep 'keyring unavailable' "$RUN/aut005.err" || say "absent — real Keychain Services, isolated in $home"
  # Item PRESENCE, not the secret. A keychain item's ACL names the application
  # that created it, so `find-generic-password -w` from this shell is refused
  # the data — but `dump-keychain` reads attributes without it, and "an item
  # with this service and account did not exist and now does" is the claim.
  head2 "keyring before"
  kc="$home/Library/Keychains/login.keychain-db"
  before_items="$(security dump-keychain "$kc" 2>/dev/null | grep -c 'com.wisprlightning.app')"
  say "items matching com.wisprlightning.app: $before_items"

  head2 "open Settings so session:changed has a live subscriber"
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  activate "$pid"
  before="$(ax pane "$pid" | grep -iE 'signed|@' | head -3)"
  say "account block before: ${before:-<nothing>}"
  shot "$pid" "$RUN/account-before.png"

  head2 "deliver a SYNTHETIC callback carrying fabricated tokens"
  url="$(python3 "$SRC/fake_callback.py")"
  say "${url%%\?*}?<fabricated tokens elided>"
  "$BIN/wlopenurl" "$pid" "$url"
  wait_log "$RUN/aut005.err" 'signed in from an auth callback' 10 && signed=0
  sleep 2

  head2 "app log"
  grep -E 'deeplink|session' "$RUN/aut005.err" || say "(nothing)"

  head2 "keyring after"
  after_items="$(security dump-keychain "$kc" 2>/dev/null | grep -c 'com.wisprlightning.app')"
  say "items matching com.wisprlightning.app: $after_items"
  security dump-keychain "$kc" 2>/dev/null | grep -A3 'com.wisprlightning.app' | grep -E 'acct|svce' | sort -u

  head2 "did session:changed reach the webview?"
  # Re-read a few times: the pane re-renders asynchronously and WebKit's
  # accessibility tree is rebuilt after the DOM, not with it.
  for _ in 1 2 3 4 5; do
    activate "$pid"
    after="$(ax pane "$pid" | grep -iE 'signed|Sign Out|@' | head -3)"
    echo "$after" | grep -q 'example.invalid' && break
    sleep 1.5
  done
  say "account block after:  ${after:-<nothing>}"
  shot "$pid" "$RUN/account-after.png"
  say "screenshots: $RUN/account-before.png  $RUN/account-after.png"
  seeded="$(sqlite3 "$home/Library/Application Support/WisprLightning/lightning.db" \
    "select count(*) from dictionary where phrase='Testinald';" 2>&1)"
  say "dictionary rows carrying the callback's first name: $seeded"
  say "(publish_session is the ONLY emitter of session:changed, and the same"
  say " function seeds the dictionary with the signed-in first name, so a row"
  say " here proves the emit path ran rather than only that tokens were stored)"

  stop_app "$pid"
  head2 "destroy the fabricated session with its throwaway keychain"
  rm -rf "$home/Library/Keychains"
  say "removed $home/Library/Keychains — nothing was written to the user's own keychain"

  if [ $signed -ne 0 ]; then
    fail AUT-005 "the callback never reached the handler"
  elif [ "$after_items" -le "$before_items" ]; then
    fail AUT-016 "no new keychain item appeared ($before_items -> $after_items)"
  elif [ "$seeded" != "1" ]; then
    fail AUT-016 "session:changed did not fire — publish_session never ran"
  elif ! echo "$after" | grep -q 'example.invalid'; then
    fail AUT-016 "the webview never re-rendered the account block, so session:changed did not reach it"
  else
    pass AUT-005 "a wisprlightning://auth/google/success URL delivered as the OS's own GURL Apple Event reached the deep-link handler and was accepted"
    pass AUT-016 "the tokens were written to a real Keychain item (service com.wisprlightning.app, account wispr-session), publish_session ran, and the live settings webview re-rendered from signed-out to the callback's account"
  fi
}

# ---------------------------------------------------------------------------
# AUT-032 — Wispr Flow watcher adoption.
#
# The watcher, if present, watches $HOME/Library/Application Support/Wispr
# Flow/. Because every app here runs under a throwaway HOME, the directory it
# watches is a throwaway one too — the user's real file is never read, moved or
# written by this row.
# ---------------------------------------------------------------------------

row_AUT_032() {
  head1 "AUT-032  a watcher event adopts a session only when the current one is invalid"
  head2 "does the watcher exist in this tree?"
  local callers watchers adopters
  callers="$(grep -rn 'wispr_flow_session_file' --include=*.rs crates src-tauri |
    grep -vc 'pub fn wispr_flow_session_file')"
  watchers="$(grep -rln 'notify::\|FSEvent\|kqueue\|RecommendedWatcher' --include=*.rs crates src-tauri | wc -l | tr -d ' ')"
  adopters="$(grep -rn '\.adopt(' --include=*.rs crates src-tauri | grep -vc 'fn adopt')"
  say "call sites of wl_core::paths::wispr_flow_session_file(): $callers"
  say "files containing a filesystem watcher:                   $watchers"
  say "call sites of Session::adopt():                          $adopters"
  say "the user's real file, never touched by this row:"
  say "  $FLOW_SESSION  $(sha "$FLOW_SESSION")"

  if [ "$callers" -eq 0 ] || [ "$watchers" -eq 0 ]; then
    fail AUT-032 "there is no Wispr Flow directory watcher in the port: wl_core::paths::wispr_flow_session_file() has zero call sites and no filesystem watcher exists in crates/ or src-tauri/. Session::adopt() is unit-tested but has no production caller. There is no behaviour to drive, so this row is not a missing test — it is missing code."
    return
  fi

  local home pid flowdir before after adopted=1 ignored=0
  home="$(make_home aut032)"
  flowdir="$home/Library/Application Support/Wispr Flow"
  # The bundle is built on demand and can predate the watcher landing, so this
  # row runs the binary built from the tree it is asserting about.
  build_app || { fail AUT-032 "could not build"; return; }
  pid="$(start_app aut032 "$BARE_BIN" "$home" "$ROOT")" || { fail AUT-032 "could not launch"; return; }
  wait_ready aut032 30 || { fail AUT-032 "app never finished setup"; return; }
  sleep 1
  say "the app created the watched directory: $([ -d "$flowdir" ] && echo yes || echo no)"
  mkdir -p "$flowdir"

  head2 "1. no valid session held — a file appearing must be adopted"
  python3 "$SRC/fake_callback.py" --file >"$flowdir/session.json"
  # The spec literal is `Wispr Lightning: Picked up session from Wispr Flow (%@)`
  # and the implementation logs `picked up a session from Wispr Flow email=…`,
  # so the match is on the part that carries the meaning rather than the
  # capitalisation. The literal mismatch is reported separately.
  wait_log "$RUN/aut032.err" 'session from Wispr Flow' 10 && adopted=0
  grep -iE 'wispr flow|adopt' "$RUN/aut032.err" || say "(nothing logged)"

  head2 "2. session now valid — a second write must change nothing"
  before="$(grep -c 'session from Wispr Flow' "$RUN/aut032.err")"
  python3 "$SRC/fake_callback.py" --file --alt >"$flowdir/session.json"
  sleep 3
  after="$(grep -c 'session from Wispr Flow' "$RUN/aut032.err")"
  say "adoption count before / after the second write: $before / $after"
  [ "$before" = "$after" ] || ignored=1
  head2 "the adopted session is the fabricated one, in the throwaway keychain"
  HOME="$home" security find-generic-password -s com.wisprlightning.app -a wispr-session -w 2>&1 |
    grep -o 'wl-e2e-fake-access[^"]*' | head -1
  stop_app "$pid"
  rm -rf "$home/Library/Keychains"

  if [ $adopted -ne 0 ]; then
    fail AUT-032 "a file written into the watched directory with no valid session was not adopted"
  elif [ $ignored -ne 0 ]; then
    fail AUT-032 "a second write was adopted even though the session was already valid"
  else
    pass AUT-032 "with no valid session, a file appearing in the watched directory is adopted and logged; with a valid session already held, a second write changes nothing"
  fi
}

# ---------------------------------------------------------------------------
# PRV-025 — the settings UI states per-provider capabilities.
# ---------------------------------------------------------------------------

row_PRV_025() {
  head1 "PRV-025  the settings UI states which capabilities each provider has"
  local home pid claims shots=0 positives=0 negatives=0 provider
  home="$(make_home prv025)"
  pid="$(start_app prv025 "$BUNDLE_BIN" "$home" "$ROOT")" || { fail PRV-025 "could not launch"; return; }
  wait_ready prv025 30 || { fail PRV-025 "app never finished setup"; return; }
  sleep 1
  tray_click "$pid" Settings >/dev/null
  sleep 2.5
  activate "$pid"
  ax press "$pid" "Transcription" >/dev/null
  sleep 1.5

  for provider in "Wispr Flow" "Deepgram"; do
    head2 "$provider — the panel the running app renders"
    ax press "$pid" "$provider" >/dev/null
    sleep 2
    claims="$(ax pane "$pid" | sed -n '/What this provider does/,$p')"
    say "$claims"
    positives=$((positives + $(echo "$claims" | grep -c 'Applies\|Adapts\|Uses on-screen\|Interprets')))
    negatives=$((negatives + $(echo "$claims" | grep -c 'Does not')))
    shot "$pid" "$RUN/provider-${provider// /-}.png" && {
      say "screenshot: $RUN/provider-${provider// /-}.png"
      shots=$((shots + 1))
    }
  done
  stop_app "$pid"

  say "capability claims read from the live UI: $positives available, $negatives unavailable"
  if [ "$shots" -ne 2 ]; then
    fail PRV-025 "could not capture both provider panels from the running app"
  elif [ "$positives" -lt 4 ] || [ "$negatives" -lt 4 ]; then
    fail PRV-025 "the panel did not state both what a provider can and cannot do"
  else
    pass PRV-025 "the running Settings window renders a per-provider capability panel; Wispr Flow states four available capabilities and Deepgram states four unavailable ones, both plus a vocabulary sentence"
  fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

ALL=(LIF-006 LIF-009 LIF-012 LIF-014 LIF-018 LIF-021 AUT-001 AUT-005 AUT-032 PRV-025)
SELECTED=("$@")
[ ${#SELECTED[@]} -eq 0 ] && SELECTED=("${ALL[@]}")

say "Wispr Lightning e2e — $(date -u +%FT%TZ)"
say "run directory: $RUN"
say "bundle:        $BUNDLE_APP"
say "bare binary:   $BARE_BIN"

[ -x "$BUNDLE_BIN" ] || { say "no bundle at $BUNDLE_BIN"; exit 1; }

# A sleeping display is not a neutral condition: window capture starts failing
# and other processes' windows drop out of the accessibility tree entirely, so
# every UI assertion below would silently degrade to "found nothing".
caffeinate -u -d -t 5400 >/dev/null 2>&1 &
CAFFEINATE_PID=$!
sleep 2

baseline
build_helpers || { say "helper build failed"; exit 1; }

running="$(pgrep -f "$ROOT/target/main/release/bundle/macos" | tr '\n' ' ')"
if [ -n "$running" ]; then
  say ""
  say "REFUSING TO RUN: an instance of the bundle is already up (pid $running)."
  say "The single-instance guard would make every launch below exit immediately."
  exit 1
fi

for row in "${SELECTED[@]}"; do
  fn="row_${row//-/_}"
  if declare -F "$fn" >/dev/null; then
    "$fn"
  else
    say "unknown row $row"
  fi
done

head1 "Summary"
for line in "${RESULTS[@]}"; do say "$line"; done

verify_untouched || FAILURES=$((FAILURES + 1))

say ""
say "artifacts: $RUN"
[ "$FAILURES" -gt 0 ] && exit 1
exit 0
