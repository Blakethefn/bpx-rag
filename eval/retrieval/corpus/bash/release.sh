---
language: shell
---
# Atomically point the live release symlink at a fully staged deployment.
activate_release() {
  ln -sfn "$1" "releases/current.next"
  mv -Tf "releases/current.next" "releases/current"
}
