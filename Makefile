# sd-widgets — common tasks

CARGO ?= cargo

.PHONY: all check build release deck daemon appimage clean

all: check

check:
	$(CARGO) check --workspace

build:
	$(CARGO) build --workspace

release:
	$(CARGO) build --release -p sdw-deck -p sdw-daemon

deck: release
	@echo "target/release/sdw-deck"

daemon: release
	@echo "target/release/sdw-daemon"

appimage:
	packaging/linux/build-appimage.sh all

clean:
	$(CARGO) clean
	rm -rf dist target/appimage-* target/macos/dmg
