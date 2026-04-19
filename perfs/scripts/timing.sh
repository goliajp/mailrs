#!/usr/bin/env bash
# usage: ./timing.sh <label> <method> <url> [data-json]
# emits one line: label  http  size_kb  dns  tcp  tls  ttfb  total
set -u
LABEL=$1; METHOD=$2; URL=$3; DATA=${4:-}
TOK=${TOKEN:-}
ARGS=(-s -o /tmp/perf-body -w '%{http_code} %{size_download} %{time_namelookup} %{time_connect} %{time_appconnect} %{time_starttransfer} %{time_total}\n' -m 30)
[[ -n $TOK ]] && ARGS+=(-H "Authorization: Bearer $TOK")
if [[ $METHOD == POST ]]; then
  ARGS+=(-X POST -H 'Content-Type: application/json' -d "$DATA")
fi
read CODE SIZE DNS TCP TLS TTFB TOT < <(curl "${ARGS[@]}" "$URL")
KB=$(awk "BEGIN{printf \"%.1f\", $SIZE/1024}")
DNSMS=$(awk "BEGIN{printf \"%.0f\", $DNS*1000}")
TCPMS=$(awk "BEGIN{printf \"%.0f\", ($TCP-$DNS)*1000}")
TLSMS=$(awk "BEGIN{printf \"%.0f\", ($TLS-$TCP)*1000}")
TTFBMS=$(awk "BEGIN{printf \"%.0f\", ($TTFB-$TLS)*1000}")
TOTMS=$(awk "BEGIN{printf \"%.0f\", $TOT*1000}")
printf '%-46s  %s  %7s KB   dns=%4sms  tcp=%4sms  tls=%4sms  ttfb=%4sms  total=%5sms\n' \
  "$LABEL" "$CODE" "$KB" "$DNSMS" "$TCPMS" "$TLSMS" "$TTFBMS" "$TOTMS"
