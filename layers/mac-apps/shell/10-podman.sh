# Podman Desktop adds /opt/podman/bin via /etc/paths.d/podman-pkg for login shells.
# Alias docker only when absent, so a later Docker Desktop install takes precedence.
if command -v podman >/dev/null 2>&1 && ! command -v docker >/dev/null 2>&1; then
  alias docker=podman
fi
