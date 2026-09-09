# Podman Desktop installs the CLI at /opt/podman/bin and puts it on PATH via
# /etc/paths.d/podman-pkg, so `podman` resolves in login shells.
#
# Take over `docker` only when podman is present and nothing else already
# provides it, so installing Docker Desktop later silently wins.
if command -v podman >/dev/null 2>&1 && ! command -v docker >/dev/null 2>&1; then
  alias docker=podman
fi
