linux-release:
	cargo build --release --target x86_64-unknown-linux-musl
	@echo "built: target/x86_64-unknown-linux-musl/release/emailpipe"

deploy: linux-release
	ssh zinc-01 'systemctl stop emailpipe'
	scp target/x86_64-unknown-linux-musl/release/emailpipe zinc-01:/data/srv/emailpipe/
	ssh zinc-01 'systemctl start emailpipe'

copy-deploy: linux-release
	scp target/x86_64-unknown-linux-musl/release/emailpipe zinc-01:/data/srv/emailpipe/

release:
	cargo build --release

test: release
	ruby scripts/test.rb
