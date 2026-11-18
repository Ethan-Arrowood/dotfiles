# SSH logins, including VS Code Remote-SSH terminals, get no agent or a
# forwarded one. This Mac's key sits in the launchd ssh-agent, so without it
# git over SSH and SSH commit signing ask for the passphrase. Use that agent.
if [[ -n ${SSH_CONNECTION:-} ]]; then
  _df_sock=$(launchctl print "gui/$UID/com.openssh.ssh-agent" 2>/dev/null \
    | awk '/SSH_AUTH_SOCK =>/ { print $3; exit }')
  [[ -S $_df_sock ]] && export SSH_AUTH_SOCK=$_df_sock
  unset _df_sock
fi
