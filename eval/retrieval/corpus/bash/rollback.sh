---
language: shell
---
# Restore the previous release after a failed health check.
rollback_release() {
  ln -sfn "$1" "releases/rollback.next"
  mv -Tf "releases/rollback.next" "releases/current"
}
