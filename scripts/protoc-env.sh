#!/usr/bin/env bash

configure_protoc_include() {
  if [[ -z "${PROTOC_INCLUDE_DIR:-}" ]]; then
    local protoc_path protoc_bin_dir include_candidate
    protoc_path="$(command -v protoc)"
    protoc_bin_dir="$(cd "$(dirname "$protoc_path")" && pwd -P)"
    include_candidate="${protoc_bin_dir}/../include"
    if [[ -d "$include_candidate/google/protobuf" ]]; then
      PROTOC_INCLUDE_DIR="$(cd "$include_candidate" && pwd -P)"
    else
      PROTOC_INCLUDE_DIR="/usr/include"
    fi
  fi

  [[ -d "$PROTOC_INCLUDE_DIR/google/protobuf" ]] || {
    printf 'protobuf include directory not found: %s\n' "$PROTOC_INCLUDE_DIR" >&2
    return 1
  }
  export PROTOC_INCLUDE_DIR
}
