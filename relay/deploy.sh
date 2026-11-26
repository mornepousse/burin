#!/usr/bin/env bash
# Puts the report relay (relay/worker.js) on Cloudflare: the Worker, its mail binding, the author's
# address as a secret, and the route https://$ZONE/api/report. From anywhere:
#   CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... relay/deploy.sh
# Run without them, it asks for the key and the account id (any shell, fish too).
# With DRY_RUN=1 it only reads: checks the key, finds the zone and says what it would do.
# The address comes from a file kept off the repository (.backup/report-email); it must be a
# verified destination of the zone's Email Routing, and Email Routing must be on.
set -uo pipefail
cd "$(dirname "$0")/.."
if [ -z "${CLOUDFLARE_API_TOKEN:-}" ]; then
    [ -t 0 ] || { echo "STOPPED: CLOUDFLARE_API_TOKEN isn't set" >&2; exit 1; }
    read -rsp "Cloudflare key (API token, hidden as you type or paste): " CLOUDFLARE_API_TOKEN
    echo
fi
if [ -z "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
    [ -t 0 ] || { echo "STOPPED: CLOUDFLARE_ACCOUNT_ID isn't set" >&2; exit 1; }
    read -rp "Cloudflare account id: " CLOUDFLARE_ACCOUNT_ID
fi
CLOUDFLARE_API_TOKEN=$(tr -d ' \r\n' <<< "$CLOUDFLARE_API_TOKEN")
CLOUDFLARE_ACCOUNT_ID=$(tr -d ' \r\n' <<< "$CLOUDFLARE_ACCOUNT_ID")
[ -n "$CLOUDFLARE_API_TOKEN" ] && [ -n "$CLOUDFLARE_ACCOUNT_ID" ] || { echo "STOPPED: the key and the account id are both needed" >&2; exit 1; }
ZONE=${RELAY_ZONE:-burin.cc}
NAME=${RELAY_NAME:-burin-relay}
FROM=${RELAY_FROM:-reports@$ZONE}
REPORT_FILE=${BURIN_REPORT_EMAIL_FILE:-.backup/report-email}
API=https://api.cloudflare.com/client/v4
AUTH="Authorization: Bearer $CLOUDFLARE_API_TOKEN"

fail() { echo "STOPPED: $*" >&2; exit 1; }
# Reads a Cloudflare answer on stdin: prints the errors and fails, or prints the field asked for.
answer() {
    python3 -c '
import json, sys
want = sys.argv[1]
try:
    d = json.load(sys.stdin)
except Exception:
    print("not an answer from Cloudflare", file=sys.stderr); sys.exit(1)
if not d.get("success"):
    for e in d.get("errors") or []:
        print("Cloudflare: %s (%s)" % (e.get("message"), e.get("code")), file=sys.stderr)
    sys.exit(1)
r = d.get("result")
if want == "zone":
    print(r[0]["id"] if r else "")
elif want == "routes":
    print("\n".join(x.get("pattern", "") for x in r or []))
' "$1"
}

[ -f "$REPORT_FILE" ] || fail "no address file at $REPORT_FILE"
TO=$(tr -d ' \n' < "$REPORT_FILE")
[ -n "$TO" ] || fail "the address file is empty"

echo "1. Checking the key..."
# An account's key (cfat_...) or a user's key.
curl -sS "$API/accounts/$CLOUDFLARE_ACCOUNT_ID/tokens/verify" -H "$AUTH" | answer ok >/dev/null 2>&1 \
    || curl -sS "$API/user/tokens/verify" -H "$AUTH" | answer ok >/dev/null \
    || fail "the key isn't accepted"
echo "   ok"

echo "2. Finding $ZONE..."
zone_id=$(curl -sS "$API/zones?name=$ZONE" -H "$AUTH" | answer zone) || fail "can't read the zone"
[ -n "$zone_id" ] || fail "$ZONE isn't on this account"
echo "   ok"

# Mail can only leave from a domain whose Email Routing is on: then its MX records point to
# Cloudflare's mail servers (checked over DNS, which the key can't read).
mx=$(curl -sS -H 'accept: application/dns-json' "https://cloudflare-dns.com/dns-query?name=$ZONE&type=MX")
if ! grep -q 'mx.cloudflare.net' <<< "$mx"; then
    echo "   WARNING: Email Routing isn't on for $ZONE (no Cloudflare MX record): the relay can't send mail."
    echo "   In the Cloudflare dashboard: $ZONE > Email > Email Routing > Enable, accept the DNS records,"
    echo "   then add and confirm your address under Destination addresses."
fi

pattern="$ZONE/api/report"
routes=$(curl -sS "$API/zones/$zone_id/workers/routes" -H "$AUTH" | answer routes) || fail "can't read the routes"

if [ "${DRY_RUN:-}" = 1 ]; then
    echo "Would upload the Worker $NAME (mail from $FROM to the address in $REPORT_FILE)."
    if grep -qxF "$pattern" <<< "$routes"; then echo "The route $pattern is already there."; else echo "Would add the route $pattern."; fi
    exit 0
fi

echo "3. Uploading the Worker $NAME..."
meta=$(mktemp)
chmod 600 "$meta"
trap 'rm -f "$meta"' EXIT
cat > "$meta" <<EOF
{"main_module":"worker.js","compatibility_date":"2026-11-01","observability":{"enabled":true},
 "bindings":[{"type":"send_email","name":"MAIL"},
             {"type":"plain_text","name":"REPORT_FROM","text":"$FROM"},
             {"type":"secret_text","name":"REPORT_TO","text":"$TO"}]}
EOF
curl -sS -X PUT "$API/accounts/$CLOUDFLARE_ACCOUNT_ID/workers/scripts/$NAME" -H "$AUTH" \
    -F "metadata=@$meta;type=application/json" \
    -F "worker.js=@relay/worker.js;type=application/javascript+module" | answer ok >/dev/null \
    || fail "the Worker wasn't uploaded (is Email Routing on for $ZONE?)"
echo "   ok"

echo "4. Route $pattern..."
if grep -qxF "$pattern" <<< "$routes"; then
    echo "   already there"
else
    curl -sS -X POST "$API/zones/$zone_id/workers/routes" -H "$AUTH" -H "Content-Type: application/json" \
        --data "{\"pattern\":\"$pattern\",\"script\":\"$NAME\"}" | answer ok >/dev/null \
        || fail "the route wasn't added"
    echo "   ok"
fi

echo "Done. Try it:"
echo "  curl -X POST https://$pattern -H 'Content-Type: application/json' -d '{\"title\":\"Essai\",\"text\":\"Premier message\"}'"
