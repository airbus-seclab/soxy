SERVICES ?= clipboard command forward ftp input socks5 stage0

VC ?= dvc svc

TARGETS_FRONTEND ?= i686-pc-windows-gnu x86_64-pc-windows-gnu i686-unknown-linux-gnu x86_64-unknown-linux-gnu
TARGETS_BACKEND ?= i686-pc-windows-gnu x86_64-pc-windows-gnu i686-unknown-linux-gnu x86_64-unknown-linux-gnu
TARGETS_STANDALONE ?= i686-pc-windows-gnu x86_64-pc-windows-gnu i686-unknown-linux-gnu x86_64-unknown-linux-gnu
TARGETS_WIN7_BACKEND ?= x86_64-win7-windows-gnu
TARGETS_WIN7_32BIT_BACKEND ?= i686-win7-windows-gnu
TARGETS_SOXYREG ?= i686-pc-windows-gnu x86_64-pc-windows-gnu

RELEASE_DIR := release
DEBUG_DIR := debug

BACKEND_RELEASE_RUST_FLAGS := --remap-path-prefix ${HOME}=/foo -Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort -C target-feature=+crt-static
BACKEND_RELEASE_BUILD_FLAGS := -Z build-std=std,panic_abort

# `build-win7` compiles with the rustc that `win7-rustc` builds from sources
# (see `win7-rustc/bootstrap.toml`). That compiler differs from the nightly one
# used for the other targets, so `BACKEND_RELEASE_RUST_FLAGS` cannot be reused
# as is:
#
#   * it is built from a release tag, hence runs on the release channel: the
#     `-Z` options are rejected, and so is `-Cpanic=immediate-abort` which
#     requires `-Zunstable-options`;
#   * it already ships the `std` for both Win7 targets, so `-Z build-std` would
#     only rebuild it needlessly.
#
# Only stable flags are used below, which keeps the Win7 build in line with
# `BACKEND_RELEASE_RUST_FLAGS` for everything the Win7 compiler does support.
# `+crt-static` is the important one: without it the artifacts depend on
# `libgcc_s_*-1.dll` and `libwinpthread-1.dll`, which the VDI agent process
# loading `soxy.dll` has no reason to be able to find.
#
# The library and the binary stay two variables (the `build-win7` recipe builds
# them separately) even though they currently share the same flags.
BACKEND_RELEASE_LIB_RUST_FLAGS := --remap-path-prefix ${HOME}=/foo -C target-feature=+crt-static
BACKEND_RELEASE_BIN_RUST_FLAGS := $(BACKEND_RELEASE_LIB_RUST_FLAGS)

# `soxyreg` is built with the nightly toolchain, so the `-Z` options are
# available, but without `BACKEND_RELEASE_BUILD_FLAGS` (no `-Z build-std`).
# `+crt-static` is the important part: `soxyreg.exe` is handed over to users.
SOXYREG_RELEASE_RUST_FLAGS := --remap-path-prefix ${HOME}=/foo -Zlocation-detail=none -C target-feature=+crt-static

TOOLCHAIN_FRONTEND_DEBUG ?= stable
TOOLCHAIN_FRONTEND_RELEASE ?= stable
TOOLCHAIN_BACKEND_DEBUG ?= stable
TOOLCHAIN_BACKEND_RELEASE ?= nightly
TOOLCHAIN_STANDALONE_DEBUG ?= stable
TOOLCHAIN_STANDALONE_RELEASE ?= stable
TOOLCHAIN_WIN7_TAG ?= 1.98.0
TOOLCHAIN_WIN7_BACKEND ?= win7-$(TOOLCHAIN_WIN7_TAG)
TOOLCHAIN_WIN7_RUST_DIR = win7-rustc
TOOLCHAIN_SOXYREG_DEBUG ?= stable
TOOLCHAIN_SOXYREG_RELEASE ?= nightly

SHELL := bash

#############

.PHONY: default
default: setup release

TOOLCHAINS := $(sort $(TOOLCHAIN_FRONTEND_DEBUG) $(TOOLCHAIN_FRONTEND_RELEASE) $(TOOLCHAIN_BACKEND_DEBUG) $(TOOLCHAIN_BACKEND_RELEASE) $(TOOLCHAIN_STANDALONE_DEBUG) $(TOOLCHAIN_STANDALONE_RELEASE) $(TOOLCHAIN_SOXYREG_DEBUG) $(TOOLCHAIN_SOXYREG_RELEASE))
TARGETS := $(sort $(TARGETS_FRONTEND) $(TARGETS_BACKEND) $(TARGETS_STANDALONE) $(TARGETS_SOXYREG))

.PHONY: setup
setup:
	@for toolchain in $(TOOLCHAINS) ; do \
	        echo ; echo "# Installing toolchain $$toolchain" ; echo ; \
		rustup toolchain add $$toolchain || exit 1 ; \
		if [[ "$$toolchain" == "nightly" ]] ; then \
			rustup component add --toolchain $$toolchain rust-src || exit 1 ; \
		fi ; \
		for target in $(TARGETS) ; do \
			echo ; echo "# Installing component $$target for $$toolchain" ; echo ; \
			rustup target add --toolchain $$toolchain $$target || exit 1 ; \
			if [[ ! "$$target" =~ "llvm" ]] ; then \
				rustup component add --toolchain $$toolchain rust-src || exit 1 ; \
			fi ; \
		done ; \
	done

.PHONY: release
release: build-release
	@for t in $(TARGETS_FRONTEND) ; do \
		for f in frontend/target/$$t/release/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(RELEASE_DIR)/frontend/$$t && \
				cp "$$f" $(RELEASE_DIR)/frontend/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_BACKEND) ; do \
		for f in backend/target/$$t/release/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(RELEASE_DIR)/backend/$$t && \
				cp "$$f" $(RELEASE_DIR)/backend/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_STANDALONE) ; do \
		for f in standalone/target/$$t/release/*standalone{,.exe} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(RELEASE_DIR)/standalone/$$t && \
				cp "$$f" $(RELEASE_DIR)/standalone/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_SOXYREG) ; do \
		for f in soxyreg/target/$$t/release/*soxyreg{,.exe} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(RELEASE_DIR)/soxyreg/$$t && \
				cp "$$f" $(RELEASE_DIR)/soxyreg/$$t/ ; \
			fi ; \
		done ; \
	done

.PHONY: debug
debug: build-debug
	@for t in $(TARGETS_FRONTEND) ; do \
		for f in frontend/target/$$t/debug/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(DEBUG_DIR)/frontend/$$t && \
				cp "$$f" $(DEBUG_DIR)/frontend/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_BACKEND) ; do \
		for f in backend/target/$$t/debug/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(DEBUG_DIR)/backend/$$t && \
				cp "$$f" $(DEBUG_DIR)/backend/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_STANDALONE) ; do \
		for f in standalone/target/$$t/debug/*standalone{,.exe} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(DEBUG_DIR)/standalone/$$t && \
				cp "$$f" $(DEBUG_DIR)/standalone/$$t/ ; \
			fi ; \
		done ; \
	done
	@for t in $(TARGETS_SOXYREG) ; do \
		for f in soxyreg/target/$$t/debug/*soxyreg{,.exe} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(DEBUG_DIR)/soxyreg/$$t && \
				cp "$$f" $(DEBUG_DIR)/soxyreg/$$t/ ; \
			fi ; \
		done ; \
	done

.PHONY: win7
win7: build-win7
	@for t in $(TARGETS_WIN7_BACKEND) ; do \
		for f in backend/target/$$t/release/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(RELEASE_DIR)/backend/$$t && \
				cp "$$f" $(RELEASE_DIR)/backend/$$t/ ; \
			fi ; \
		done ; \
	done

# Same as `win7` but restricted to the 32 bits (i686) Windows 7 backend.
# `win7-32bit-release` is an alias of `win7-32bit`, so that both profiles can
# be built with the same `win7-32bit-<profile>` recipe.
.PHONY: win7-32bit win7-32bit-release
win7-32bit win7-32bit-release:
	$(MAKE) win7 TARGETS_WIN7_BACKEND="$(TARGETS_WIN7_32BIT_BACKEND)"

.PHONY: win7-debug
win7-debug: build-win7-debug
	@for t in $(TARGETS_WIN7_BACKEND) ; do \
		for f in backend/target/$$t/debug/*soxy{,.dll,.exe,.so} ; do \
			if [[ -f "$$f" ]] ; then \
				mkdir -p $(DEBUG_DIR)/backend/$$t && \
				cp "$$f" $(DEBUG_DIR)/backend/$$t/ ; \
			fi ; \
		done ; \
	done

# Debug flavour of `win7-32bit`: the backend is built without the release
# optimizations and with the `log` feature, which turns every
# `common::{trace,debug,info,warn,error}!` into an actual log line. That is the
# build to use when diagnosing why soxy does not talk to the VDI (the release
# one strips the logs away).
.PHONY: win7-32bit-debug
win7-32bit-debug:
	$(MAKE) win7-debug TARGETS_WIN7_BACKEND="$(TARGETS_WIN7_32BIT_BACKEND)"

.PHONY: distclean
distclean: clean
	@rm -Rf ${RELEASE_DIR} ${DEBUG_DIR}
	@$(MAKE) -C $(TOOLCHAIN_WIN7_RUST_DIR) $@

#############

FEATURES_SERVICES := $(addprefix service-,$(SERVICES))
FEATURES_SERVICES := $(strip $(FEATURES_SERVICES))
FEATURES_SERVICES := $(shell echo "$(FEATURES_SERVICES)" | sed 's/ /,/g')
FEATURES_SERVICES := "$(FEATURES_SERVICES)"

FEATURES_VC := $(strip $(VC))
FEATURES_VC := $(shell echo "$(FEATURES_VC)" | sed 's/ /,/g')
FEATURES_VC := "$(FEATURES_VC)"

.PHONY: build-release
build-release:
	@for t in $(TARGETS_FRONTEND) ; do \
		echo ; echo "# Building release frontend ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_FRONTEND_RELEASE)" ; echo ; \
		(cd frontend && cargo +$(TOOLCHAIN_FRONTEND_RELEASE) build --release --features log,$(FEATURES_VC),$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_BACKEND) ; do \
		echo ; echo "# Building release backend ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_BACKEND_RELEASE)" ; echo ; \
		(cd backend && RUSTFLAGS="$(BACKEND_RELEASE_RUST_FLAGS)" cargo +$(TOOLCHAIN_BACKEND_RELEASE) build --release --features $(FEATURES_VC),$(FEATURES_SERVICES) --target $$t $(BACKEND_RELEASE_BUILD_FLAGS) && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_STANDALONE) ; do \
		echo ; echo "# Building release standalone ($(SERVICES)) for $$t with $(TOOLCHAIN_STANDALONE_RELEASE)" ; echo ; \
		(cd standalone && cargo +$(TOOLCHAIN_STANDALONE_RELEASE) build --release --features log,$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_SOXYREG) ; do \
		echo ; echo "# Building release soxyreg for $$t with $(TOOLCHAIN_SOXYREG_RELEASE)" ; echo ; \
		(cd soxyreg && RUSTFLAGS="$(SOXYREG_RELEASE_RUST_FLAGS)" cargo +$(TOOLCHAIN_SOXYREG_RELEASE) build --release --target $$t && cd ..) || exit 1 ; \
	done

.PHONY: build-debug
build-debug:
	@for t in $(TARGETS_FRONTEND) ; do \
		echo ; echo "# Building debug frontend ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_FRONTEND_DEBUG)" ; echo ; \
		(cd frontend && cargo +$(TOOLCHAIN_FRONTEND_DEBUG) build --features log,$(FEATURES_VC),$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_BACKEND) ; do \
		echo ; echo "# Building debug backend ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_BACKEND_DEBUG)" ; echo ; \
		(cd backend && cargo +$(TOOLCHAIN_BACKEND_DEBUG) build --features log,$(FEATURES_VC),$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_STANDALONE) ; do \
		echo ; echo "# Building debug standalone ($(SERVICES)) for $$t with $(TOOLCHAIN_STANDALONE_DEBUG)" ; echo ; \
		(cd standalone && cargo +$(TOOLCHAIN_STANDALONE_DEBUG) build --features log,$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_SOXYREG) ; do \
		echo ; echo "# Building debug soxyreg for $$t with $(TOOLCHAIN_SOXYREG_DEBUG)" ; echo ; \
		(cd soxyreg && cargo +$(TOOLCHAIN_SOXYREG_DEBUG) build --target $$t && cd ..) || exit 1 ; \
	done

.PHONY: build-win7
build-win7:
	@echo "Checking the backend toolchain for a Win7 target"
	$(MAKE) -C $(TOOLCHAIN_WIN7_RUST_DIR)   \
		TAG=$(TOOLCHAIN_WIN7_TAG)           \
		TOOLCHAIN=$(TOOLCHAIN_WIN7_BACKEND) \
		TARGETS="$(TARGETS_WIN7_BACKEND)"
	@for t in $(TARGETS_WIN7_BACKEND) ; do  \
		echo ; echo "# Building release backend library ($(SERVICES)) for $$t with $(TOOLCHAIN_WIN7_BACKEND)" ; echo ; \
		(cd backend && \
		 RUSTFLAGS="$(BACKEND_RELEASE_LIB_RUST_FLAGS)" \
		 cargo +$(TOOLCHAIN_WIN7_BACKEND) build --lib --release --features $(FEATURES_VC),$(FEATURES_SERVICES) --target $$t \
		) || exit 1 ; \
		echo ; echo "# Building release backend binary ($(SERVICES)) for $$t with $(TOOLCHAIN_WIN7_BACKEND)" ; echo ; \
		(cd backend && \
		 RUSTFLAGS="$(BACKEND_RELEASE_BIN_RUST_FLAGS)" \
		 cargo +$(TOOLCHAIN_WIN7_BACKEND) build --bins --release --features $(FEATURES_VC),$(FEATURES_SERVICES) --target $$t \
		) ; \
	done

.PHONY: build-win7-debug
build-win7-debug:
	@echo "Checking the backend toolchain for a Win7 target"
	$(MAKE) -C $(TOOLCHAIN_WIN7_RUST_DIR)   \
		TAG=$(TOOLCHAIN_WIN7_TAG)           \
		TOOLCHAIN=$(TOOLCHAIN_WIN7_BACKEND) \
		TARGETS="$(TARGETS_WIN7_BACKEND)"
	@for t in $(TARGETS_WIN7_BACKEND) ; do  \
		echo ; echo "# Building debug backend library ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_WIN7_BACKEND)" ; echo ; \
		(cd backend && \
		 cargo +$(TOOLCHAIN_WIN7_BACKEND) build --lib --features log,$(FEATURES_VC),$(FEATURES_SERVICES) --target $$t \
		) || exit 1 ; \
		echo ; echo "# Building debug backend binary ($(VC)) ($(SERVICES)) for $$t with $(TOOLCHAIN_WIN7_BACKEND)" ; echo ; \
		(cd backend && \
		 cargo +$(TOOLCHAIN_WIN7_BACKEND) build --bins --features log,$(FEATURES_VC),$(FEATURES_SERVICES) --target $$t \
		) ; \
	done

.PHONY: build-frontend-native
build-frontend-native:
	echo ; echo "# Building debug frontend ($(VC)) ($(SERVICES))" ; echo
	(cd frontend && cargo build --features log,$(FEATURES_VC),$(FEATURES_SERVICES) && cd ..) || exit 1
	echo ; echo "# Building release frontend ($(VC)) ($(SERVICES))" ; echo
	(cd frontend && cargo build --release --features log,$(FEATURES_VC),$(FEATURES_SERVICES) && cd ..) || exit 1

.PHONY: build-standalone-native
build-standalone-native:
	echo ; echo "# Building debug standalone ($(SERVICES))" ; echo
	(cd standalone && cargo build --features log,$(FEATURES_SERVICES) && cd ..) || exit 1
	echo ; echo "# Building release standalone ($(SERVICES))" ; echo
	(cd standalone && cargo build --release --features log,$(FEATURES_SERVICES) && cd ..) || exit 1


#############

.PHONY: check clippy
check clippy:
	@for t in $(TARGETS_FRONTEND) ; do \
		echo ; echo "# Clippy on frontend for $$t with $(TOOLCHAIN_FRONTEND_DEBUG)" ; echo ; \
		(cd common && cargo +$(TOOLCHAIN_FRONTEND_DEBUG) $@ --features frontend,log,$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
		(cd frontend && cargo +$(TOOLCHAIN_FRONTEND_DEBUG) $@ --features $(FEATURES_VC),$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_BACKEND) ; do \
		echo ; echo "# Clippy on backend for $$t with $(TOOLCHAIN_BACKEND_DEBUG)" ; echo ; \
		(cd common && cargo +$(TOOLCHAIN_BACKEND_DEBUG) $@ --features backend,$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
		(cd backend && cargo +$(TOOLCHAIN_BACKEND_DEBUG) $@ --features $(FEATURES_VC),$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_STANDALONE) ; do \
		echo ; echo "# Clippy on standalone for $$t with $(TOOLCHAIN_STANDALONE_DEBUG)" ; echo ; \
		(cd common && cargo +$(TOOLCHAIN_STANDALONE_DEBUG) $@ --features frontend,backend,log,$(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
		(cd standalone && cargo +$(TOOLCHAIN_STANDALONE_DEBUG) $@ --features $(FEATURES_SERVICES) --target $$t && cd ..) || exit 1 ; \
	done
	@for t in $(TARGETS_SOXYREG) ; do \
		echo ; echo "# Clippy on soxyreg for $$t with $(TOOLCHAIN_SOXYREG_DEBUG)" ; echo ; \
		(cd soxyreg && cargo +$(TOOLCHAIN_SOXYREG_DEBUG) $@ --target $$t && cd ..) || exit 1 ; \
	done


.PHONY: cargo-fmt
cargo-fmt:
	@for c in common frontend backend standalone soxyreg ; do \
		(cd $$c && $@ +nightly && cd ..) || exit 1 ; \
	done

print-%:
	@echo $*=$($*)

%:
	@for c in common frontend backend standalone soxyreg ; do \
		(cd $$c && cargo $@ && cd ..) || exit 1 ; \
	done
