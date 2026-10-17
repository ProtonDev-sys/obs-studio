#!/usr/bin/env bash
# Exercise the packaged AppRun, libcurl and OBS initializer against a private
# TLS endpoint (#1085). The AppDir remains untouched. The private client AppDir
# copies its launcher and links its libraries; the client replaces only the GUI.
# Usage: test-apprun-curl.sh <AppDir> <obs-appimage-curl-test> <output-dir>
set -euo pipefail
umask 077

APPDIR_SOURCE="$(realpath "${1:?AppDir}")"
CLIENT="$(realpath "${2:?obs-appimage-curl-test}")"
OUT="$(realpath -m "${3:?output-dir}")"
mkdir -p "$OUT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/obs-appimage-curl.XXXXXX")"
SERVER_PID=""
cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill -TERM "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT

FIXTURE="$WORK/appdir"
mkdir -p "$FIXTURE/usr/bin"
cp "$APPDIR_SOURCE/AppRun" "$FIXTURE/AppRun"
chmod 755 "$FIXTURE/AppRun"
for entry in "$APPDIR_SOURCE"/*; do
    case "${entry##*/}" in AppRun | usr) continue ;; esac
    ln -s "$entry" "$FIXTURE/${entry##*/}"
done
ln -s "$APPDIR_SOURCE/usr/lib" "$FIXTURE/usr/lib"
ln -s "$APPDIR_SOURCE/usr/share" "$FIXTURE/usr/share"
GUI_NAME=obs
if [ -f "$APPDIR_SOURCE/usr/bin/obs-studio-rust-gui" ]; then GUI_NAME=obs-studio-rust-gui; fi
install -m755 "$CLIENT" "$FIXTURE/usr/bin/$GUI_NAME"

# Fresh certificates avoid expiration dates or the host trust store becoming
# test inputs. OpenSSL 1.1.1 (the glibc 2.28 builder) supports these options.
for ca in trusted wrong; do
    openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=OBS test $ca CA" \
        -addext 'basicConstraints=critical,CA:TRUE' \
        -keyout "$WORK/$ca.key" -out "$WORK/$ca.pem" >"$OUT/$ca-ca.log" 2>&1
done
openssl req -newkey rsa:2048 -nodes -subj /CN=localhost \
    -keyout "$WORK/server.key" -out "$WORK/server.csr" >"$OUT/server-csr.log" 2>&1
cat >"$WORK/extensions.cnf" <<'EOF'
subjectAltName=DNS:localhost
extendedKeyUsage=serverAuth
basicConstraints=critical,CA:FALSE
EOF
openssl x509 -req -in "$WORK/server.csr" -CA "$WORK/trusted.pem" -CAkey "$WORK/trusted.key" \
    -set_serial 1 -days 2 -extfile "$WORK/extensions.cnf" -out "$WORK/server.pem" \
    >"$OUT/server-certificate.log" 2>&1

# The server owns its port before handing it to the clients. Its pipe gives a
# readiness event; the timeout reports a broken fixture instead of hanging CI.
coproc TLS_SERVER {
    exec "$FIXTURE/usr/bin/$GUI_NAME" serve "$WORK/server.pem" "$WORK/server.key" 2>"$OUT/server.log"
}
SERVER_PID=$TLS_SERVER_PID
if ! read -r -t 10 URL <&"${TLS_SERVER[0]}"; then
    cat "$OUT/server.log"
    echo 'FAIL: TLS fixture did not report a listening port' >&2
    exit 1
fi
case "$URL" in https://localhost:[0-9]*/) ;; *) echo "FAIL: invalid TLS fixture URL: $URL" >&2; exit 1 ;; esac

FAILED=0
run_client() {
    local name="$1"
    shift
    set +e
    timeout --kill-after=2s 10s "$@" >"$OUT/$name.stdout" 2>"$OUT/$name.stderr"
    RUN_RC=$?
    set -e
    cat "$OUT/$name.stderr"
}
expect_result() {
    local name="$1" expected="$2"
    case " $expected " in
    *" $RUN_RC "*) echo "PASS: $name (curl $RUN_RC)" ;;
    *) echo "FAIL: $name expected curl $expected, got $RUN_RC" >&2; FAILED=1 ;;
    esac
}

# Ordinary launches, including an empty package variable, retain libcurl's
# own CA defaults. Compare with the original initializer on the same library.
run_client native-raw env -u OBS_CURL_CA_BUNDLE SSL_CERT_FILE="$WORK/trusted.pem" \
    "$FIXTURE/usr/bin/$GUI_NAME" raw "$URL"
RAW_RC=$RUN_RC
expect_result native-raw '0 60 77'
run_client native-unset env -u OBS_CURL_CA_BUNDLE SSL_CERT_FILE="$WORK/trusted.pem" \
    "$FIXTURE/usr/bin/$GUI_NAME" get "$URL"
expect_result native-unset "$RAW_RC"
run_client native-empty env OBS_CURL_CA_BUNDLE= SSL_CERT_FILE="$WORK/trusted.pem" \
    "$FIXTURE/usr/bin/$GUI_NAME" get "$URL"
expect_result native-empty "$RAW_RC"

run_client trusted-ca env -u OBS_CURL_CA_BUNDLE SSL_CERT_FILE="$WORK/trusted.pem" \
    "$FIXTURE/AppRun" get "$URL"
expect_result trusted-ca 0
run_client wrong-ca env -u OBS_CURL_CA_BUNDLE SSL_CERT_FILE="$WORK/wrong.pem" \
    "$FIXTURE/AppRun" get "$URL"
expect_result wrong-ca 60
run_client wrong-hostname env -u OBS_CURL_CA_BUNDLE SSL_CERT_FILE="$WORK/trusted.pem" \
    "$FIXTURE/AppRun" get "${URL/localhost/127.0.0.1}"
# Older libcurl reports hostname mismatches as 51, newer versions as 60.
expect_result wrong-hostname '51 60'
exit "$FAILED"
