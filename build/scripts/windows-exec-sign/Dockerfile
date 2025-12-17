FROM debian:bullseye

RUN apt-get update \
    && apt-get -y install \
        openssl \
        libengine-pkcs11-openssl \
        gnutls-bin \
        xxd \
        osslsigncode

ADD cmd.sh /cmd.sh

CMD ["/cmd.sh"]
