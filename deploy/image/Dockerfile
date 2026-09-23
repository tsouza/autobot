# The operator image behind `just operator-image`. The build context holds only the operator
# binary, compiled on the host; nothing compiles here. The base is pinned by digest (the
# multi-platform index of the `nonroot` tag), which `autobot_devtools::image` checks.
FROM gcr.io/distroless/cc-debian13:nonroot@sha256:54df941ed0d06a1bd95ef5e0ce391fd8d9f94b64782dc9a60062727849ee3f97
COPY autobot-operator /usr/local/bin/autobot-operator
ENTRYPOINT ["/usr/local/bin/autobot-operator"]
