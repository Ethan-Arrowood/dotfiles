# Python via uv — it manages interpreters and virtualenvs, so no pyenv.
path_prepend "$HOME/.local/bin"
command -v uv >/dev/null 2>&1 && alias venv='uv venv'
