**NB: This code is alpha and not ready for production use.**

# Overrides

_Overrides_ produces Istio config to enable a request to opt into different versions of Services along a chain.

Eg given a chain of Services: `Gateway -> S1 -> S2 -> S3`, each running two versions: `v1` and `v2`
* It may be desirable to send traffic to `S2:v2`, _in context_, ie sent via `v1` of the other services

_Overrides_ allows you to direct a request down the chain of `v1`s, taking a detour to a specific `v2`, thus testing that new version in the most representative context possible.
This is as simple as `curl -H "x-override: service-3:v2" https://<gateway>`.
You can take multiple detours by setting multiple values for the header: `/usr/bin/curl -v -H "x-override: service-3:v2" -H "x-override: service-5:v2" https://<gateway>`.

Operation is quite simple:
* A `DestinationRule` defines a _subset_ for each version of the service
* A `VirtualService` matches `x-override` header values of the form `<service name>:<version>` and sends them to the appropriate subset

These resources can, of course, be made manually, but that's quite a tedious exercise with lots of services, especially if they're chaning all the time.
This repo contains code that automates that.
This repo contains:
* A library (_overrides_, `/src/lib.rs`) which contains code for generating the DR and VS for a given Service and versions set.
* A CLI, _override-generator_ (`/src/bin/override-generator/main.rs`) which does a one-shot read of the current namespace and prints the Istio config for it.
* An Operator, _override-operator_ (`/src/bin/override-operator/main.rs`) which watches Services and Pods, reconsiling them to the needed Istio config.

This code is alpha at best, and makes some big assumptions about your cluster; see the assumptions and bugs below.

### override-generator
Override Generator reads the kubeconfig's current namespace.

See the resources with
```bash
cargo run
```

Apply to a cluster with
```bash
cargo run 2>/dev/null | kubectl apply -f -
```

### override-operator
It can be run locally, and will connect to your kubeconfig's current context, and operate on the current namespace.
The user that runs it will need various permissions, as seen in `/deploy/override-operator.yaml`

```bash
cargo run --bin override operator
```

It can also be run in-cluster, and a [pre-built container image is available](https://hub.docker.com/repository/docker/mtinside/override-operator).
You can deploy it with the supplied resource definitions.
```bash
kubectl apply -f ./deploy/override-operator.yaml
```

## Assumptions
The current alpha code doesn't attempt to play nice with any other Istio config that exists, and has rigid requirements about the environment it's applied to.
It basically needs its own namespace to own at this point.

* All Services in the namespace will be processed
  * The `kubernetes` Service is ignored, but all others must meet the requiremets
  * Every Pod selected by each Service must have a `version` label (this is used to build the DR's subsets)
  * At least one of those Pods must have `version: v1` (a default route is added to the `v1` subset for traffic with no `x-override` header)
* There are no other `VirtualServices` or `DestinationRules` for the Service's hostname
* Services need to forward the `x-override` header.

## Bugs
* Watch Pods?
* Attach the VS for the first service in the chain to the gateway. This needs
  * Knowing which service(s) should be exposed
  * Knowing the gateway resource name to use
  * Knowing the external vhost name

## Future work
* "Support gitops" - Flux annotation to not touch our resources?
* Stash the override requests in a JWT, following Lyft's design. This would mean services don't have to forward `x-override`.

## Design choices
* Kopium pre-generation of strongly-typed resource models (vs the other options: https://kube.rs/controllers/object/)
