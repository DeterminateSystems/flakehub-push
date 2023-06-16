FROM ubuntu:latest
RUN apt update && apt install curl git -y
RUN curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install linux \
  --extra-conf "sandbox = false" \
  --init none \
  --no-confirm
ENV PATH="${PATH}:/nix/var/nix/profiles/default/bin"

RUN mkdir -p /nxfr-push
COPY flake.nix flake.lock .cargo Cargo.lock Cargo.toml README.md .gitignore /nxfr-push/
COPY src /nxfr-push/src
COPY .git /nxfr-push/.git

RUN nix profile install /nxfr-push#nxfr-push
RUN mkdir -p /github/workspace

ENTRYPOINT [ "nxfr-push" ]