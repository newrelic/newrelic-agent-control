#!/bin/bash
set -eo pipefail

print_usage() {
  printf -- "Usage: %s\n" "$(basename "${0}")"
  printf -- "-f: Package file to sign\n"
  printf -- "-h: Print help page\n"
}

while getopts 'f:h' flag
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
        *)
          print_usage
          exit 1
        ;;
    esac
done

if [ -z "${file_name}" ] ; then
    echo "-f package not provided" >&2
    print_usage
    exit 1
fi

if [ ! -f "${file_name}"  ]; then
    echo "file: ${file_name} doesn't exist" >&2
    print_usage
    exit 1
fi

prepare() {
    # prepare RPM's
    echo "===> Create .rpmmacros to sign rpm's from Goreleaser"
    echo "%_gpg_name ${GPG_MAIL}" >> ~/.rpmmacros
    echo "%_signature gpg" >> ~/.rpmmacros
    echo "%_gpg_path /root/.gnupg" >> ~/.rpmmacros
    echo "%_gpgbin /usr/bin/gpg" >> ~/.rpmmacros
    echo "%__gpg_sign_cmd   %{__gpg} gpg --no-verbose --no-armor --batch --pinentry-mode loopback --passphrase ${GPG_PASSPHRASE} --no-secmem-warning -u "%{_gpg_name}" -sbo %{__signature_filename} %{__plaintext_filename}" >> ~/.rpmmacros

    echo "===> Importing GPG private key from GHA secrets..."
    printf %s ${GPG_PRIVATE_KEY_BASE64} | base64 -d | gpg --batch --import -

    echo "===> Importing GPG signature, needed from Goreleaser to verify signature"
    gpg --export -a ${GPG_MAIL} > /tmp/RPM-GPG-KEY-${GPG_MAIL}
    rpm --import /tmp/RPM-GPG-KEY-${GPG_MAIL}

    # prepare DEB's
    GNUPGHOME="/root/.gnupg"
    echo "${GPG_PASSPHRASE}" > "${GNUPGHOME}/gpg-passphrase"
    echo "passphrase-file ${GNUPGHOME}/gpg-passphrase" >> "$GNUPGHOME/gpg.conf"
    echo 'allow-loopback-pinentry' >> "${GNUPGHOME}/gpg-agent.conf"
    echo 'pinentry-mode loopback' >> "${GNUPGHOME}/gpg.conf"
    echo 'use-agent' >> "${GNUPGHOME}/gpg.conf"
    echo RELOADAGENT | gpg-connect-agent
}

sign_rpm() {
    rpm_file="${1}"
    echo "===> Signing ${rpm_file}"
    rpm --addsign "${rpm_file}"
    echo "===> Sign verification ${rpm_file}"
    rpm -v --checksig "${rpm_file}"
}

sign_deb() {
    deb_file="${1}"
    echo "===> Signing ${deb_file}"
    debsigs --sign=origin --verify --check -v -k "${GPG_MAIL}" "${deb_file}"
}

sign_file() {
    targz_file="${1}"
    echo "===> Signing ${targz_file}"
    gpg --sign --armor --detach-sig "${targz_file}"
    echo "===> Sign verification ${targz_file}"
    gpg --verify ${targz_file}.asc "${targz_file}"
}

prepare

if [[ "${file_name}" =~ .*\.(rpm) ]]; then
    sign_rpm "${file_name}"
elif [[ "${file_name}" =~ .*\.(deb) ]]; then
    sign_deb "${file_name}"
elif [[ "${file_name}" =~ .*\.(tar.gz) ]]; then
    sign_file "${file_name}"
fi
