# This Makefile is used to rebuild a rust toolchain.
# It is intendended for the Win7 target that is no longer a Tier1 architecture
# for rustc. Because it is now Tier3, we need to rebuild it ourselves.

# Git tag of the compiler version to build
TAG ?= 1.87.0

# Name of the toolchain being built
TOOLCHAIN ?= win7-$(TAG)

HOST ?= $(shell rustc -vV | grep "^host:" | awk '{print $$2}')
TARGETS ?= i686-win7-windows-gnu x86_64-win7-windows-gnu

# Rustc git repo
RUST_GIT ?= https://github.com/rust-lang/rust.git

# Directory in which to checkout rustc source
RUST_SRC ?= $(CURDIR)/rust-src

# Directory in which to build the compiler
RUST_BUILD ?= $(CURDIR)/rust-build

# Directory containing the stage2 once it is built
RUST_STAGE2_DIR ?= $(RUST_BUILD)/build/$(HOST)/stage2

SHELL := bash

.PHONY: build
build: setup
	@rustup toolchain list | grep -q $(TOOLCHAIN) || {             \
		echo Building toolchain: $(TOOLCHAIN);                    \
		pushd $(RUST_BUILD)                                       \
		&& $(abspath $(RUST_SRC))/x build                         \
			--host=$(HOST)                                        \
			--target=$(shell tr ' ' ',' <<< "$(TARGETS) $(HOST)") \
		&& popd                                                   \
		&& $(MAKE) register-toolchain                             \
			TOOLCHAIN=$(TOOLCHAIN)                                \
			RUST_STAGE2_DIR=$(RUST_STAGE2_DIR);                   \
	}

.PHONY: setup
setup:
	@[ -d $(RUST_SRC) ] || {                                       \
		mkdir -p $(dir $(RUST_SRC))                               \
		&& git clone $(RUST_GIT) $(RUST_SRC)                      \
		&& cd $(RUST_SRC)                                         \
		&& git checkout $(TAG);                                   \
	}
	@mkdir -p $(RUST_BUILD)
	@cp bootstrap.toml $(RUST_BUILD)

.PHONY: register-toolchain
register-toolchain:
	@rustup toolchain link $(TOOLCHAIN) $(RUST_STAGE2_DIR)

print-%:
	@echo $*=$($*)

.PHONY: clean
clean:
	@cd $(RUST_BUILD) && ./x clean

.PHONY: distclean
distclean:
	@rm -Rf $(RUST_BUILD) $(RUST_SRC)
