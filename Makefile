RELEASE_DIR:=release
DEBUG_DIR:=debug

#BACKEND_WINDOWS32_RUST_FLAGS:=--remap-path-prefix ${HOME}=/foo -Ctarget-feature=+crt-static -Zlocation-detail=none
BACKEND_WINDOWS64_RUST_FLAGS:=--remap-path-prefix ${HOME}=/foo -Ctarget-feature=+crt-static -Zlocation-detail=none
#BACKEND_WINDOWS32_BUILD_FLAGS:=-Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort
BACKEND_WINDOWS64_BUILD_FLAGS:=-Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort

.PHONY: setup
setup:
	rustup toolchain add stable nightly
	rustup target add --toolchain nightly i686-pc-windows-gnu x86_64-pc-windows-gnu
	rustup target add x86_64-unknown-linux-gnu i686-pc-windows-gnu x86_64-pc-windows-gnu
	rustup component add rust-src --toolchain nightly-x86_64-unknown-linux-gnu

.PHONY: release
release: build-release
	mkdir -p $(RELEASE_DIR)/frontend/win32
	cp frontend/target/i686-pc-windows-gnu/release/*.dll $(RELEASE_DIR)/frontend/win32/
	mkdir -p $(RELEASE_DIR)/frontend/win64
	cp frontend/target/x86_64-pc-windows-gnu/release/*.dll $(RELEASE_DIR)/frontend/win64/
	mkdir -p $(RELEASE_DIR)/frontend/linux64
	cp frontend/target/x86_64-unknown-linux-gnu/release/lib*.so $(RELEASE_DIR)/frontend/linux64/
#	mkdir -p $(RELEASE_DIR)/backend/win32
#	cp backend/target/i686-pc-windows-gnu/release/*.dll $(RELEASE_DIR)/backend/win32/
#	cp backend/target/i686-pc-windows-gnu/release/*.exe $(RELEASE_DIR)/backend/win32/
	mkdir -p $(RELEASE_DIR)/backend/win64
	cp backend/target/x86_64-pc-windows-gnu/release/*.dll $(RELEASE_DIR)/backend/win64/
	cp backend/target/x86_64-pc-windows-gnu/release/*.exe $(RELEASE_DIR)/backend/win64/
	mkdir -p $(RELEASE_DIR)/standalone/win32
	cp standalone/target/i686-pc-windows-gnu/release/*standalone.exe $(RELEASE_DIR)/standalone/win32/
	mkdir -p $(RELEASE_DIR)/standalone/win64
	cp standalone/target/x86_64-pc-windows-gnu/release/*standalone.exe $(RELEASE_DIR)/standalone/win64/
	mkdir -p $(RELEASE_DIR)/standalone/linux64
	cp standalone/target/x86_64-unknown-linux-gnu/release/*standalone $(RELEASE_DIR)/standalone/linux64/

.PHONY: debug
debug: build-debug
	mkdir -p $(DEBUG_DIR)/frontend/win32
	cp frontend/target/i686-pc-windows-gnu/debug/*.dll $(DEBUG_DIR)/frontend/win32/
	mkdir -p $(DEBUG_DIR)/frontend/win64
	cp frontend/target/x86_64-pc-windows-gnu/debug/*.dll $(DEBUG_DIR)/frontend/win64/
	mkdir -p $(DEBUG_DIR)/frontend/linux64
	cp frontend/target/x86_64-unknown-linux-gnu/debug/lib*.so $(DEBUG_DIR)/frontend/linux64/
	mkdir -p $(DEBUG_DIR)/backend/win32
	cp backend/target/i686-pc-windows-gnu/debug/*.dll $(DEBUG_DIR)/backend/win32/
	cp backend/target/i686-pc-windows-gnu/debug/*.exe $(DEBUG_DIR)/backend/win32/
	mkdir -p $(DEBUG_DIR)/backend/win64
	cp backend/target/x86_64-pc-windows-gnu/debug/*.dll $(DEBUG_DIR)/backend/win64/
	cp backend/target/x86_64-pc-windows-gnu/debug/*.exe $(DEBUG_DIR)/backend/win64/
	mkdir -p $(DEBUG_DIR)/standalone/win32
	cp standalone/target/i686-pc-windows-gnu/debug/*standalone.exe $(DEBUG_DIR)/standalone/win32/
	mkdir -p $(DEBUG_DIR)/standalone/win64
	cp standalone/target/x86_64-pc-windows-gnu/debug/*standalone.exe $(DEBUG_DIR)/standalone/win64/
	mkdir -p $(DEBUG_DIR)/standalone/linux64
	cp standalone/target/x86_64-unknown-linux-gnu/debug/*standalone $(DEBUG_DIR)/standalone/linux64/

.PHONY: distclean
distclean: clean
	rm -rf ${RELEASE_DIR} ${DEBUG_DIR}

#############

.PHONY: build-release
build-release:
	cd frontend ; cargo build --release --features log --target i686-pc-windows-gnu
	cd frontend ; cargo build --release --features log --target x86_64-pc-windows-gnu
	cd frontend ; cargo build --release --features log --target x86_64-unknown-linux-gnu
#	cd backend ; RUSTFLAGS="$(BACKEND_WINDOWS32_RUST_FLAGS)" cargo +nightly build --release --target i686-pc-windows-gnu $(BACKEND_WINDOWS32_BUILD_FLAGS)
	cd backend ; RUSTFLAGS="$(BACKEND_WINDOWS64_RUST_FLAGS)" cargo +nightly build --release --target x86_64-pc-windows-gnu $(BACKEND_WINDOWS64_BUILD_FLAGS)
	cd standalone ; cargo build --release --features log --target i686-pc-windows-gnu
	cd standalone ; cargo build --release --features log --target x86_64-pc-windows-gnu
	cd standalone ; cargo build --release --features log --target x86_64-unknown-linux-gnu

.PHONY: build-debug
build-debug:
	cd frontend ; cargo build --features log --target i686-pc-windows-gnu
	cd frontend ; cargo build --features log --target x86_64-pc-windows-gnu
	cd frontend ; cargo build --features log --target x86_64-unknown-linux-gnu
	cd backend ; cargo build --features log --target i686-pc-windows-gnu
	cd backend ; cargo build --features log --target x86_64-pc-windows-gnu
	cd standalone ; cargo build --features log --target i686-pc-windows-gnu
	cd standalone ; cargo build --features log --target x86_64-pc-windows-gnu
	cd standalone ; cargo build --features log --target x86_64-unknown-linux-gnu

#############

.PHONY: clippy
clippy:
	cd common ; cargo $@
	cd frontend ; cargo $@ --target i686-pc-windows-gnu
	cd frontend ; cargo $@ --target x86_64-pc-windows-gnu
	cd frontend ; cargo $@ --target x86_64-unknown-linux-gnu
	cd backend ; cargo $@ --target i686-pc-windows-gnu
	cd backend ; cargo $@ --target x86_64-pc-windows-gnu
	cd backend ; cargo $@ --target x86_64-unknown-linux-gnu
	cd standalone ; cargo $@ --target x86_64-pc-windows-gnu
	cd standalone ; cargo $@ --target x86_64-unknown-linux-gnu

.PHONY: cargo-fmt
cargo-fmt:
	cd common ; $@
	cd frontend ; $@
	cd backend ; $@
	cd standalone ; $@

%:
	cd common ; cargo $@
	cd frontend ; cargo $@
	cd backend ; cargo $@
	cd standalone ; cargo $@
