#!/usr/bin/env bash
set -euo pipefail
mkdir -p logs
printf '%s template app placeholder
' "$(date -Is)" | tee -a logs/app.log
sleep infinity
