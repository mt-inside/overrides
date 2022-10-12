default:
	@just --list

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