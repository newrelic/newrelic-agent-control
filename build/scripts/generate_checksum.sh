#!/bin/bash
set -eo pipefail

print_usage() {
  printf -- "Usage: %s\n" $(basename "${0}")
  printf -- "-o: Output file for generated checksum\n"
  printf -- "-f: File to generate checksum for\n"
}

while getopts 'o:f:h' flag
do
    if [ -z "${OPTARG}" ]; then
      continue
    fi
    case "${flag}" in
        h)
          print_usage
          exit 0
        ;;
        o)
          OUTPUT_FILE="${OPTARG}"
          continue
        ;;
        f)
         FILE="${OPTARG}"
         continue
        ;;
        *)
          print_usage
          exit 1
        ;;
    esac
done

if [ -z "${FILE}" ]; then
    echo "-f file not provided\n" >&2
    print_usage
    exit 1
fi

# if OUTPUT_FILE was provided we use it as destination filename.
if [ -n "${OUTPUT_FILE}" ]; then
    output_file="./${OUTPUT_FILE}"
else
    output_file="./${FILE}.sum"
fi

echo -n > "${output_file}"

echo "Processing file: ${FILE}, creating ${output_file}"
sha256sum "${FILE}" | awk -F ' ' '{gsub(".*/", "", $2); print $1 "  " $2}' >> "${output_file}"
