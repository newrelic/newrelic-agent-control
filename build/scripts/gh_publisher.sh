#!/bin/bash
#
#
# Upload file to GH Release assets
#
#

print_usage() {
  printf -- "Usage: %s\n" "$(basename "${0}")"
  printf -- "-f: File to uplload\n"
  printf -- "-t: tag of the release\n"
  printf -- "-h: Print help page\n"
}

while getopts 'f:t:h' flag
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
        t)
         tag="${OPTARG}"
         continue
        ;;
        *)
          print_usage
          exit 1
        ;;
    esac
done

# delete_asset_by_name is used when we want to re-upload an asset that failed or was partially published.
delete_asset_by_name() {
  artifact="${1}"
  
  repo=$(git config --get remote.origin.url | sed -En "s/.*github.com[:/]//p")
  repo=${repo%.*}

  assets_url=$(gh api "repos/${repo}/releases/tags/${tag}" --jq '[.assets_url] | @tsv')
  if [ "${?}" -ne 0 ]; then
    exit 1
  fi

  page=1
  while [ "${page}" -lt 20 ]; do
    echo "fetching assets page: ${page}..."
    assets=$(gh api "${assets_url}?page=${page}" --jq '.[] | [.url,.name] | @tsv' | tee)
    if [ "${?}" -ne 0 ]; then
      exit 2
    fi

    if [ "${assets}" = "" ]; then
      break
    fi

    while IFS= read -r asset;
    do
      assetArray=("${asset}")
      if [ "${assetArray[1]}" = "${artifact}"  ]; then
        gh api -X DELETE "${assetArray[0]}"
        if [ "${?}" -ne 0 ]; then
          exit 3
        fi
        echo "deleted ${artifact}, retry..."
        return
      fi
    done < <(echo "$assets")
  ((page++))
  done
  echo "no assets found to delete with the name: ${artifact}"
}

MAX_ATTEMPTS=20
ATTEMPTS=$MAX_ATTEMPTS

echo "===> Uploading to GH ${tag}: ${file_name}"

while [ "${ATTEMPTS}" -gt 0 ];do
  gh release upload "${tag}" "${file_name}" --clobber

  if [[ "${?}" -eq 0 ]];then
    echo "===> uploaded  ${file_name}"
    break
  fi

  set -e
    delete_asset_by_name "$(basename "${file_name}")"
  set +e
  sleep 3s
  (( ATTEMPTS-- ))
done

if [ "${ATTEMPTS}" -eq 0 ];then
  echo "too many attempts to upload ${file_name}"
  exit 1
fi

