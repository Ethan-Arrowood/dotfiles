# uv manages Python interpreters and virtualenvs.
path_prepend "$HOME/.local/bin"
command -v uv >/dev/null 2>&1 && alias venv='uv venv'
