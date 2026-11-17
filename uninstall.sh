#!/usr/bin/env bash
# Desinstala la TUI o la GUI de ZeroAgents sin tocar los datos ni la otra.
#
#   ./uninstall.sh gui    quita ~/.local/bin/zeroagents-gui, su .desktop y su icono
#   ./uninstall.sh tui    quita zeroagents.desktop; el comando `zeroagents` se queda si la GUI
#                         está instalada (recoge los hooks de los que lee), o con --keep-cli
#
# Nunca borra ~/.local/state/zeroagents (historial, proyectos, estado) ni ~/.claude.
# Si quitas la TUI sin la GUI, el comando `zeroagents` y sus hooks se desinstalan
# también (`zeroagents uninstall` + `uv tool uninstall`), salvo con --keep-cli.
set -euo pipefail

BIN="${HOME}/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
export PATH="$BIN:$PATH"

say() { printf '%s\n' "$*"; }
usage() { sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

KEEP_CLI=0; WHAT=""
for arg in "$@"; do
  case "$arg" in
    tui|gui) WHAT="$arg" ;;
    --keep-cli) KEEP_CLI=1 ;;
    -h|--help) usage 0 ;;
    *) usage 2 ;;
  esac
done

case "$WHAT" in
  gui)
    rm -f "$BIN/zeroagents-gui" "$APPS/zeroagents-gui.desktop" "$ICONS/zeroagents-gui.svg"
    say "GUI desinstalada. La TUI y los datos siguen intactos."
    ;;
  tui)
    rm -f "$APPS/zeroagents.desktop"
    say "· Lanzador de la TUI quitado ($APPS/zeroagents.desktop)"
    if [ -x "$BIN/zeroagents-gui" ]; then
      say "· La GUI sigue instalada: se conserva el comando zeroagents (recoge los hooks de Claude Code)."
    elif [ "$KEEP_CLI" = 1 ]; then
      say "· Comando zeroagents conservado (--keep-cli)."
    elif command -v zeroagents >/dev/null 2>&1; then
      zeroagents uninstall || true
      command -v uv >/dev/null 2>&1 && uv tool uninstall zeroagents || true
      say "· Comando zeroagents y hooks desinstalados."
    fi
    say "TUI desinstalada. La GUI y los datos siguen intactos."
    ;;
  *) usage 2 ;;
esac
