linux-release:
	cargo build --release --target x86_64-unknown-linux-musl
	@echo "built: target/x86_64-unknown-linux-musl/release/emailpipe"

deploy: linux-release
	scp target/x86_64-unknown-linux-musl/release/emailpipe zinc-01:/data/srv/emailpipe/

release:
	cargo build --release

test: release
	ruby scripts/test.rb
