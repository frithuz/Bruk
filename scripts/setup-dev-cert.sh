#!/usr/bin/env bash
set -euo pipefail

# One-time setup for a local macOS self-signed code-signing certificate
# ("Bruk Local Dev") so debug builds of Bruk keep a stable designated
# requirement across rebuilds and avoid repeated macOS Keychain prompts.

CERT_NAME="${BRUK_DEV_SIGN_IDENTITY:-Bruk Local Dev}"
LOGIN_KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "note: dev code-signing setup is only needed on macOS."
  exit 0
fi

if security find-identity -v -p codesigning 2>/dev/null | grep -Fq "\"$CERT_NAME\""; then
  echo "Code-signing identity \"$CERT_NAME\" is already installed and valid."
  security find-identity -v -p codesigning | grep -F "\"$CERT_NAME\""
  exit 0
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cat > "$TMP_DIR/cert.cfg" <<EOF
[req]
distinguished_name = req_distinguished_name
x509_extensions = v3_codesign
prompt = no

[req_distinguished_name]
CN = $CERT_NAME

[v3_codesign]
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
basicConstraints = critical, CA:false
EOF

openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$TMP_DIR/key.pem" \
  -out "$TMP_DIR/cert.pem" \
  -days 3650 \
  -config "$TMP_DIR/cert.cfg" \
  2>/dev/null

# Package into a temporary PKCS#12 bundle for Keychain import.
P12_PASS="$(openssl rand -hex 16)"
openssl pkcs12 -export \
  -inkey "$TMP_DIR/key.pem" \
  -in "$TMP_DIR/cert.pem" \
  -out "$TMP_DIR/dev-cert.p12" \
  -passout "pass:$P12_PASS" \
  -legacy 2>/dev/null || \
openssl pkcs12 -export \
  -inkey "$TMP_DIR/key.pem" \
  -in "$TMP_DIR/cert.pem" \
  -out "$TMP_DIR/dev-cert.p12" \
  -passout "pass:$P12_PASS"

security import "$TMP_DIR/dev-cert.p12" \
  -k "$LOGIN_KEYCHAIN" \
  -P "$P12_PASS" \
  -T /usr/bin/codesign

# Trust the self-signed certificate for code signing (macOS may prompt for confirmation).
security add-trusted-cert -p codeSign -k "$LOGIN_KEYCHAIN" "$TMP_DIR/cert.pem"

echo ""
echo "Installed local code-signing identity \"$CERT_NAME\"."
echo "Local `task run` and `task package` runs will now sign Bruk with --identifier dev.frithuz.bruk."
security find-identity -v -p codesigning | grep -F "\"$CERT_NAME\"" || true
