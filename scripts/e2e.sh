#!/usr/bin/env bash
#
# End-to-end harness: drives the REAL Wispr Lightning application.
#
# Everything here runs the packaged .app bundle and asserts through the macOS
# accessibility API, CoreGraphics' window list, the application's own log, and
# the filesystem. Nothing is mocked and no application source is modified.
#
# Rows: LIF-006, LIF-012, LIF-014, LIF-018.
#
# Why a script rather than Rust integration tests: these cases exercise the
# installed macOS UI, process lifecycle, and bundle-only behavior.
#
# Usage:
#   scripts/e2e.sh                 # every row
#   scripts/e2e.sh LIF-006 LIF-018 # named rows only
#
# Safety contract:
#   * each app run uses a throwaway HOME for persistent application state;
#   * only processes this script started are ever signalled. The user's own
#     /Applications/Wispr Lightning.app is left strictly alone.

set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

export PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

BUNDLE_APP="${WL_BUNDLE:-$ROOT/target/dx/wispr-lightning/bundle/macos/macos/WisprLightning.app}"
BUNDLE_BIN="$BUNDLE_APP/Contents/MacOS/wispr-lightning"

RUN="$(cd "$(mktemp -d /tmp/wl-e2e.XXXXXX)" && pwd -P)"
BIN="$RUN/bin"
SRC="$ROOT/scripts/e2e"
mkdir -p "$BIN"


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
# Never `open -a`, never match by process name. The user's installed app has the
# same bundle identifier, so launches here use absolute paths and track only
# the PIDs returned by `start_app`.
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


# ---------------------------------------------------------------------------
# Throwaway HOME for application state.
# ---------------------------------------------------------------------------

make_home() {
  local home="$RUN/home-$1"
  mkdir -p "$home"
  echo "$home"
}

# ---------------------------------------------------------------------------
# Post-run process safety
# ---------------------------------------------------------------------------


verify_no_strays() {
  head1 "Post-run verification — app process state"
  local bad=0 stray
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
  local pid
  for pid in "${STARTED_PIDS[@]:-}"; do
    [ -n "$pid" ] && stop_app "$pid"
  done
  [ -n "$CAFFEINATE_PID" ] && kill "$CAFFEINATE_PID" 2>/dev/null
  return 0
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

build_helpers() {
  head2 "compiling the window and accessibility helpers"
  swiftc -O -o "$BIN/wlwindows" "$SRC/wlwindows.swift" || return 1
  swiftc -O -o "$BIN/wlax" "$SRC/wlax.swift" || return 1
  say "helpers built into $BIN"
  ax trusted || return 1
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
    fail LIF-012 "no shutdown line — the process ended without completing shutdown"
  elif [ "$integrity" != "ok" ]; then
    fail LIF-012 "integrity_check returned: $integrity"
  elif [ "$wal_after" != "0" ]; then
    fail LIF-012 "tray Quit exits 0, logs the shutdown line and leaves the database uncorrupted (integrity_check ok) — but the handle is NOT closed: the WAL is still $wal_after bytes un-checkpointed."
  else
    pass LIF-012 "tray Quit logs the shutdown line and exits 0; the WAL is checkpointed to 0 bytes and integrity_check is ok, so the handle was closed rather than abandoned"
  fi
}

# ---------------------------------------------------------------------------
# LIF-014 — single instance.
# ---------------------------------------------------------------------------

row_LIF_014() {
  head1 "LIF-014  a second launch exits instead of starting a second tray icon"
  head2 "no same-identifier app is running; launch the first test instance"

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
  head1 "LIF-018  bundled sounds resolve when launched with cwd=/"
  local home pid packs wav_files

  home="$(make_home lif018)"
  pid="$(start_app lif018 "$BUNDLE_BIN" "$home" /)" || { fail LIF-018 "could not launch"; return; }
  wait_ready lif018 || { fail LIF-018 "app never finished setup"; return; }
  sleep 1
  if grep -q 'no sounds directory' "$RUN/lif018.err"; then
    grep 'sounds' "$RUN/lif018.err"
    stop_app "$pid"
    fail LIF-018 "running from / broke resource resolution"
    return
  fi
  packs="$(find "$BUNDLE_APP/Contents/Resources/resources/sounds" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
  wav_files="$(find "$BUNDLE_APP/Contents/Resources/resources/sounds" -type f -name '*.wav' | wc -l | tr -d ' ')"
  stop_app "$pid"
  if [ "$packs" != "4" ] || [ "$wav_files" != "21" ]; then
    fail LIF-018 "bundle contains $packs sound packs and $wav_files WAV files; expected 4 packs and all 21 sound assets"
  else
    pass LIF-018 "the app starts with cwd=/ without a missing-sounds warning; its selected bundle resource root contains 4 packs and all 21 WAV files"
  fi
}


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

ALL=(LIF-006 LIF-012 LIF-014 LIF-018)
SELECTED=("$@")
[ ${#SELECTED[@]} -eq 0 ] && SELECTED=("${ALL[@]}")

say "Wispr Lightning e2e — $(date -u +%FT%TZ)"
say "run directory: $RUN"
say "bundle:        $BUNDLE_APP"

[ -x "$BUNDLE_BIN" ] || { say "no bundle at $BUNDLE_BIN"; exit 1; }

running="$(pgrep -f "$(dirname "$BUNDLE_APP")" | tr '\n' ' ')"
installed="$(pgrep -f '/Applications/Wispr Lightning.app/Contents/MacOS/wispr-lightning' | tr '\n' ' ')"
if [ -n "$running$installed" ]; then
  say ""
  say "REFUSING TO RUN: another instance shares the app's single-instance key."
  say "Bundle pids: ${running:-<none>}; installed app pids: ${installed:-<none>}."
  say "Close it before testing; this harness never stops the user's app."
  exit 1
fi
# A sleeping display is not a neutral condition: window capture starts failing
# and other processes' windows drop out of the accessibility tree entirely, so
# every UI assertion below would silently degrade to "found nothing".
caffeinate -u -d -t 5400 >/dev/null 2>&1 &
CAFFEINATE_PID=$!
sleep 2

build_helpers || { say "helper build failed"; exit 1; }


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

verify_no_strays || FAILURES=$((FAILURES + 1))

say ""
say "artifacts: $RUN"
[ "$FAILURES" -gt 0 ] && exit 1
exit 0
