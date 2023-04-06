set dotenv-load

default:
	@just --list --unsorted --color=always

DH_USER := "mtinside"
REPO := "docker.io/" + DH_USER + "/override-operator"
TAG := `cargo metadata --format-version 1 --no-deps -q | jq -r '.packages[0].version'`
TAGD := `cargo metadata --format-version 1 --no-deps -q | jq -r '.packages[0].version'`
ARCHS := "aarch64" # ",amd64,armv7"

# install build dependencies
install-tools:
	# Need https://github.com/kube-rs/kopium/issues/87
	cargo install --git https://github.com/kube-rs/kopium --branch main -- kopium

generate:
	#!/usr/bin/env bash
	yq="docker run -i --rm mikefarah/yq"
	src=./crd-all.gen.yaml
	curl -s https://raw.githubusercontent.com/istio/istio/master/manifests/charts/base/crds/crd-all.gen.yaml > ${src}
	# mirror_percent and mirrorPercent clash, as they both render to the same Rust field name. Remove one. Doesn't matter which, as they're both deprecated; it's mirrorPercentage now
	cat ${src} | ${yq} eval 'del(.spec.versions[].schema.openAPIV3Schema.properties.spec.properties.http.items.properties.mirror_percent)' | sponge ${src}
	cat ${src} | ${yq} eval 'del(.spec.versions[].schema.openAPIV3Schema.properties.spec.properties.jwtRules.items.properties.jwks_uri)' | sponge ${src}
	dir=src/istio
	mkdir -p ${dir}
	crds="$(cat ${src} | ${yq} eval-all '[.metadata.name] | .[]')"
	echo '// Generated file' > ${dir}/mod.rs
	for crd in ${crds}
	do
		echo "Processing ${crd}"
		vers="$(cat ${src} | ${yq} eval-all 'select(.metadata.name=="'${crd}'") | [.spec.versions[].name] | .[]')"
		for ver in ${vers}
		do
			basenam=${crd//./_}_${ver}
			echo "Outputting ${ver} > ${basenam}"
			# Can't -D default, because while it works for the structs (which contain Options, which have an impl of Default), but not the enums, which don't mark #[Default]
			cat ${src} | ${yq} eval 'select(.metadata.name=="'${crd}'") | del(.spec.versions[] | select(.name != "'${ver}'"))' | kopium --auto -D Default -f - | grep -v 'kube(status' > ${dir}/${basenam}.rs
			echo "pub mod ${basenam};" >> ${dir}/mod.rs
		done
	done

lint:
	cargo clippy -- -D warnings # warn=>err

# Builds both binaries
build: lint
	cargo build

run-generator: lint
	cargo run
run-operator: lint
	cargo run --bin override-operator

MELANGE := "docker run --pull always --rm --privileged -v ${PWD}:/work cgr.dev/chainguard/melange:latest"
APKO    := "docker run --pull always --rm -v ${PWD}:/work cgr.dev/chainguard/apko:latest"
APKO_SH := "docker run --pull always --rm -v ${PWD}:/work --entrypoint sh cgr.dev/chainguard/apko:latest"

melange:
	# keypair to verify the package between melange and apko. apko will very quietly refuse to find our apk if these args aren't present
	{{MELANGE}} keygen
	{{MELANGE}} build --arch {{ARCHS}} --signing-key /work/melange.rsa melange.yaml
# TODO: sent a default logging level for each one
image-load-dev:
	{{APKO}} build --keyring-append melange.rsa.pub --debug --arch {{ARCHS}} apko.yaml {{REPO}}:dev override-operator.tar
	docker load < override-operator.tar
image-publish-dev:
	{{APKO_SH}} -c \
		'echo "'${DH_TOKEN}'" | apko login docker.io -u {{DH_USER}} --password-stdin && \
		apko publish apko.yaml {{REPO}}:dev --keyring-append melange.rsa.pub --arch {{ARCHS}}'
image-load-release:
	{{APKO}} build --keyring-append melange.rsa.pub --debug --arch {{ARCHS}} apko.yaml {{REPO}}:{{TAG}} override-operator.tar
	docker load < override-operator.tar
image-publish-release:
	{{APKO_SH}} -c \
		'echo "'${DH_TOKEN}'" | apko login docker.io -u {{DH_USER}} --password-stdin && \
		apko publish apko.yaml {{REPO}}:{{TAG}} --keyring-append melange.rsa.pub --arch {{ARCHS}}'

sbom-show:
	docker sbom {{REPO}}:{{TAG}}

cosign-sign:
	# Experimental includes pushing the signature to a Rekor transparency log, default: rekor.sigstore.dev
	COSIGN_EXPERIMENTAL=1 cosign sign {{REPO}}:{{TAG}}
cosign-verify:
	COSIGN_EXPERIMENTAL=1 cosign verify {{REPO}}:{{TAG}} | jq .
