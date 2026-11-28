# A small menu over the notice storybook scenes, for a web terminal such as
# ttyd: `ttyd -W -p 7681 bash preview.sh <storybook-binary>`.
BIN=${1:?usage: bash preview.sh <storybook-binary>}
while true; do
  clear
  cat <<'MENU'
Notice cell storybook (synthetic fixtures; every action is a mock)

  1  Gallery: every notice, ← → width, s state
  2  Startup: CLI update, then repo invitation
  3  Startup: adapter stderr sign-in message
  4  Startup: adapter update, then feature tip

In a startup scene, type and press Enter to send, or press Tab or Esc on an
empty prompt. Ctrl+R replays the scene; Ctrl+C returns here.
MENU
  printf '\nChoose 1-4: '
  read -r -n 1 choice
  case $choice in
    1) "$BIN" --scene gallery --width 80 ;;
    2) "$BIN" --scene startup --notice cli --also invitation ;;
    3) "$BIN" --scene startup --notice sign-in ;;
    4) "$BIN" --scene startup --notice adapter --also tip ;;
  esac
done
