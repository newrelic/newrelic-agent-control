#!/bin/bash
set -eo pipefail

#
#
# GPG signing for packages packages
#
#

print_usage() {
  printf -- "Usage: %s\n" "$(basename "${0}")"
  printf -- "-f: File to sign\n"
  printf -- "-m: gpg mail address\n"
  printf -- "-p: gpg passphrase\n"
  printf -- "-k: gpg private key base64\n"
  printf -- "-b: Force build of the docker image\n"
  printf -- "-h: Print help page\n"
}

current_dir="$( dirname $( readlink -f ${BASH_SOURCE[0]} ) )"

while getopts 'f:m:p:k:bh' flag
do
    case "${flag}" in
        h)
          print_usage
          exit 0
        ;;
        f)
         file_name="${OPTARG}"
         continue
        ;;
        m)
         gpg_mail="${OPTARG}"
         continue
        ;;
        p)
         gpg_passphrase="${OPTARG}"
         continue
        ;;
        k)
         gpg_private_key_base64="${OPTARG}"
         continue
        ;;
        b)
         should_build=true
         continue
        ;;
        *)
          print_usage
          exit 1
        ;;
    esac
done

if [ -z "${file_name}" ]; then
    echo "-f file_name not provided\n" >&2
    print_usage
    exit 1
fi

image_name="assets_signer"

# Build docker image only if doesn't already exist or explicitly requested.
if [ "$(docker images -q ${image_name} 2> /dev/null)" = "" ] || [ "${should_build}" = true ]; then
    docker build -t "${image_name}" "${current_dir}/."
fi

if [ -z "${gpg_mail}" ]; then
    echo "-m gpg_mail not provided\n" >&2
    print_usage
    exit 1
fi

if [ -z "${gpg_passphrase}" ]; then
    echo "-p gpg_passphrase not provided\n" >&2
    print_usage
    exit 1
fi

if [ -z "${gpg_private_key_base64}" ]; then
    echo "-k gpg_private_key_base64 not provided\n" >&2
    print_usage
    exit 1
fi

docker run --rm -t --name "assets_signer" \
    -v "${current_dir}/../../../:/srv/workdir" \
    -w /srv/workdir \
    -e GPG_MAIL="${gpg_mail}" \
    -e GPG_PASSPHRASE="${gpg_passphrase}" \
    -e GPG_PRIVATE_KEY_BASE64="${gpg_private_key_base64}" \
    "${image_name}" -f "${file_name}"
