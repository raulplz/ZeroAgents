#!/usr/bin/env bash
# Instala ZeroAgents: la terminal (TUI), la GUI o ambas. Sin sudo ni dependencias globales.
#
#   ./install.sh tui      comando `zeroagents` (uv tool, editable) + zeroagents.desktop
#   ./install.sh gui      ~/.local/bin/zeroagents-gui + zeroagents-gui.desktop («ZeroAgents (GUI)»)
#   ./install.sh ambas    las dos
#   --build               (gui) recompila el binario aunque ya exista
#
# Cada una se instala por separado y comparten los mismos datos (docs/datos.md).
# Nunca se tocan los datos del usuario. Un .desktop ya existente no se sobrescribe
# (en la TUI) o se guarda antes como .bak (en la GUI).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="${HOME}/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
export PATH="$HOME/.cargo/bin:$BIN:$PATH"

say() { printf '%s\n' "$*"; }
die() { printf 'Error: %s\n' "$*" >&2; exit 1; }

usage() { sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

have_cli() { command -v zeroagents >/dev/null 2>&1; }

# El comando `zeroagents` (Python) recoge los hooks y la statusline de Claude Code;
# tanto la TUI como la GUI leen lo que escribe, así que la GUI también lo necesita.
ensure_cli() {
  if have_cli; then
    say "· Comando zeroagents ya instalado: $(command -v zeroagents)"
    return
  fi
  command -v uv >/dev/null 2>&1 || die "Hace falta uv para instalar el comando zeroagents (https://docs.astral.sh/uv/)."
  uv tool install --editable "$ROOT"
  say "· Comando zeroagents instalado con uv tool"
}

hooks_hint() {
  if have_cli && ! zeroagents doctor 2>/dev/null | grep -q "Hooks instalados: \([0-9]*\)/\1\$"; then
    say "  Si aún no lo hiciste, conecta Claude Code con: zeroagents install"
  fi
}

# Copia un archivo de forma atómica (el binario puede estar en ejecución).
put() { # origen destino modo
  local tmp; tmp="$(mktemp "$2.XXXXXX")"
  cp "$1" "$tmp"; chmod "$3" "$tmp"; mv -f "$tmp" "$2"
}

install_tui() {
  say "Instalando la TUI…"
  ensure_cli
  mkdir -p "$APPS"
  if [ -e "$APPS/zeroagents.desktop" ]; then
    say "· $APPS/zeroagents.desktop ya existe: no se toca"
  else
    cp "$ROOT/extras/zeroagents.desktop" "$APPS/zeroagents.desktop"
    say "· Lanzador creado: $APPS/zeroagents.desktop"
  fi
  hooks_hint
}

install_gui() {
  say "Instalando la GUI…"
  local built="$ROOT/gui/target/release/zeroagents-gui"
  if [ "${REBUILD:-0}" = 1 ] || [ ! -x "$built" ]; then
    say "· Compilando (cargo tauri build --no-bundle)…"
    command -v cargo-tauri >/dev/null 2>&1 || command -v cargo >/dev/null 2>&1 \
      || die "Falta la toolchain de Rust (cargo, cargo-tauri y trunk). Ver gui/README o mise.toml."
    local run=(); command -v mise >/dev/null 2>&1 && run=(mise exec --)
    (cd "$ROOT/gui" && "${run[@]}" cargo tauri build --no-bundle)
  fi
  [ -x "$built" ] || die "No encuentro $built"
  ensure_cli
  mkdir -p "$BIN" "$APPS" "$ICONS"
  put "$built" "$BIN/zeroagents-gui" 755
  put "$ROOT/gui/src-tauri/icons/zeroagents.svg" "$ICONS/zeroagents-gui.svg" 644
  local desktop="$APPS/zeroagents-gui.desktop" tmp
  tmp="$(mktemp)"
  cat > "$tmp" <<DESK
[Desktop Entry]
Type=Application
Name=ZeroAgents (GUI)
GenericName=Equipos de agentes
Comment=Ventana Frost para vigilar agentes de Claude Code y seguir equipos (modo Work)
Exec=$BIN/zeroagents-gui
Icon=zeroagents-gui
Terminal=false
Categories=Development;
Keywords=agentes;claude;equipo;work;zero;
StartupNotify=true
StartupWMClass=zeroagents-gui
DESK
  if [ -e "$desktop" ] && ! cmp -s "$tmp" "$desktop"; then
    cp "$desktop" "$desktop.bak"
  fi
  mv -f "$tmp" "$desktop"; chmod 644 "$desktop"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" 2>/dev/null || true
  say "· Binario: $BIN/zeroagents-gui"
  say "· Lanzador: $desktop"
  hooks_hint
}

REBUILD=0; WHAT=""
for arg in "$@"; do
  case "$arg" in
    tui|gui|ambas) WHAT="$arg" ;;
    --build) REBUILD=1 ;;
    -h|--help) usage 0 ;;
    *) usage 2 ;;
  esac
done
case "$WHAT" in
  tui) install_tui ;;
  gui) install_gui ;;
  ambas) install_tui; install_gui ;;
  *) usage 2 ;;
esac
say "Hecho. Los datos (~/.local/state/zeroagents) los comparten la TUI y la GUI."
