set dotenv-load

default:
	@just --list --unsorted --color=always

DH_USER := "mtinside"
REPO := "docker.io/" + DH_USER + "/override-operator"
TAG := `cargo metadata --format-version 1 --no-deps -q | jq -r '.packages[0].version'`
TAGD := `cargo metadata --format-version 1 --no-deps -q | jq -r '.packages[0].version'`
ARCHS := "linux/arm64" #,linux/amd64,linux/arm/v7"
CGR_ARCHS := "aarch64" # "amd64,aarch64,armv7"

# install build dependencies
install-tools:
	# Need https://github.com/kube-rs/kopium/issues/87
	cargo install --git https://github.com/kube-rs/kopium --branch main -- kopium

generate:
	#!/usr/bin/env bash
	#src=https://raw.githubusercontent.com/istio/istio/master/manifests/charts/base/crds/crd-all.gen.yaml
	src=./crd-all.gen.yaml
	dir=src/istio
	mkdir -p ${dir}
	crds="$(cat ${src} | yq eval-all '[.metadata.name] | .[]' | grep -vi 'requestauth' | grep -vi 'peerauth')"
	for crd in ${crds}
	do
		echo "Outputting ${crd}"
		# Can't -D default, because while it works for the structs (which contain Options, which have an impl of Default), but not the enums, which don't mark #[Default]
		cat ${src} | yq eval 'select(.metadata.name=="'${crd}'")' | kopium -A -D Default -f - --api-version v1beta1 | grep -v 'kube(status' > ${dir}/${crd//./_}.rs
	done
	echo "${crds//./_}" | sed 's/\(.*\)/pub mod \1;/' > ${dir}/mod.rs
	#no_docs=$(curl -sSL https://raw.githubusercontent.com/istio/istio/master/manifests/charts/base/crds/crd-all.gen.yaml | yq eval-all '[.] | length')
	# Last document is empty, because of a trailing ---
	#for (( i=0; i<${no_docs}-1; i++ ))
		#curl -sSL https://raw.githubusercontent.com/istio/istio/master/manifests/charts/base/crds/crd-all.gen.yaml | yq eval 'select(di == '${i}')' | kopium -Af - > istio-$i.rs

lint:
	cargo clippy -- -D warnings # warn=>err

build: lint
	cargo build

run: lint
	cargo run

image-dev: lint
	docker build -f Dockerfile.dev . --tag {{REPO}}:dev --push --platform linux/arm64

image-release: lint
	docker build -f Dockerfile.release . --tag {{REPO}}:{{TAG}} --push --platform {{ARCHS}}

melange:
	# keypair to verify the package between melange and apko. apko will very quietly refuse to find our apk if these args aren't present
	docker run --rm -v "${PWD}":/work cgr.dev/chainguard/melange keygen
	docker run --privileged --rm -v "${PWD}":/work cgr.dev/chainguard/melange build --arch {{CGR_ARCHS}} --signing-key melange.rsa melange.yaml
package-cgr: melange
	docker run --rm -v "${PWD}":/work cgr.dev/chainguard/apko build -k melange.rsa.pub --debug --build-arch {{CGR_ARCHS}} apko.yaml {{REPO}}:{{TAG}} override-operator.tar
	docker load < override-operator.tar
publish-cgr: melange
	docker run --rm -v "${PWD}":/work --entrypoint sh cgr.dev/chainguard/apko --debug -c \
		'echo "'${DH_TOKEN}'" | apko login docker.io -u {{DH_USER}} --password-stdin && \
		apko publish apko.yaml {{REPO}}:{{TAG}} -k melange.rsa.pub --arch {{CGR_ARCHS}}'

sbom-show:
	docker sbom {{REPO}}:{{TAG}}

cosign-sign:
	# Experimental includes pushing the signature to a Rekor transparency log, default: rekor.sigstore.dev
	COSIGN_EXPERIMENTAL=1 cosign sign {{REPO}}:{{TAG}}
cosign-verify:
	COSIGN_EXPERIMENTAL=1 cosign verify {{REPO}}:{{TAG}} | jq .
