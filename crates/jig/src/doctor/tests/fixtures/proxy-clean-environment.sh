#!/bin/bash
if [ "$#" -ne 3 ] || [ "$1" != proxy ] || [ "$2" != list ] || [ "$3" != --json ]; then
  exit 19
fi
if [ ! -f .jig.toml ]; then
  exit 20
fi
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ]; then
  exit 21
fi
if declare -F jig_doctor_proxy_poison >/dev/null; then
  exit 22
fi
case "$-" in *x*|*v*) exit 23 ;; esac
shopt -q extglob && exit 24
case "$PS4" in *JIG_DOCTOR_PROXY_PS4_POISON*) exit 25 ;; esac
[ "$JIG_DOCTOR_PROXY_ORDINARY" = preserved ] || exit 26
printf '%s\n' '{"ok":true,"running":false,"routes":[]}'
