#!/bin/sh
case "$*" in
  *"rejection classifier"*) printf '%s\n' '{"classification":"address_mismatch","confidence":0.91,"summary":"address differs","key_points":[],"jurisdiction":"GDPR"}' ;;
  *"email classifier"*) printf '%s\n' '{"classification":"confirmed","confidence":0.93,"summary":"deletion confirmed","extracted_fields":{"ticket":"T-42"}}' ;;
esac
