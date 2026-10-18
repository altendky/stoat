#!/usr/bin/env sh
# Verify that binaries are statically linked
#
# Runs on Ubuntu build runners after building with musl target.
# Uses 'file' command which correctly identifies static vs dynamic linking.
#
# Note: We use 'file' instead of 'ldd' because musl's ldd is a simple wrapper
# that outputs the loader path even for static binaries, unlike glibc's ldd
# which says "not a dynamic executable" for static binaries.

set -eux

# Install file command if not present
if ! command -v file >/dev/null 2>&1; then
	if command -v apt-get >/dev/null 2>&1; then
		sudo apt-get update && sudo apt-get install -y file
	else
		echo "ERROR: 'file' not available and no supported package manager found" >&2
		exit 1
	fi
fi

# Cargo's intermediate directory layout is not stable. Nextest records the
# actual executable paths used to produce this archive on every toolchain.
METADATA=$(tar -xOf "${1:-target/nextest-archive.tar.zst}" target/nextest/binaries-metadata.json)
BINARIES=$(printf '%s\n' "$METADATA" | jq -er '
	."rust-binaries"
	| if type != "object" or length == 0 then error("No test binaries in archive metadata") else .[] end
	| ."binary-path"
	| if type != "string" or length == 0 then error("Invalid test binary path") else . end
')

printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
	if [ ! -f "$bin" ] || [ ! -x "$bin" ]; then
		echo "ERROR: Test binary is missing or not executable: $bin"
		exit 1
	fi
	description=$(file -b -- "$bin")
	if ! printf '%s\n' "$description" | grep -q 'ELF.*executable'; then
		echo "ERROR: Expected an ELF test executable: $bin ($description)"
		exit 1
	fi
	if printf '%s\n' "$description" | grep -qE 'statically linked|static-pie linked'; then
		echo "OK: $bin is statically linked"
	else
		echo "ERROR: $bin is dynamically linked ($description)"
		exit 1
	fi
done
