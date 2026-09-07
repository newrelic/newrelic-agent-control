FROM debian:trixie-slim

RUN apt-get update \
    && apt-get -y install \
        rpm \
        gnupg2 \
        gpg-agent \
        debsigs \
        curl

ADD entrypoint.sh /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
