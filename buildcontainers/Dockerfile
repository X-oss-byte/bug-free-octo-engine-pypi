ARG BASE_IMAGE

FROM ${BASE_IMAGE} AS base

RUN \
  curl -sL https://deb.nodesource.com/setup_14.x | bash - \
  && apt-get install -y nodejs \
  && apt-get clean \
  && rm -rf /var/lib/apt/lists/* \
    /tmp/* \
    /var/tmp/* \
    rm -rf /usr/share/man/* \
    /usr/share/doc

ARG GIT_REPO
LABEL org.opencontainers.image.source $GIT_REPO
