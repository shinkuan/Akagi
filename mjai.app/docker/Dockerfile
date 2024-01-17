FROM ubuntu:22.04

RUN mkdir -p /workspace
WORKDIR /workspace
ENV HOME /workspace

ENV TZ=Asia/Tokyo
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

RUN apt-get -y update && apt-get -y install curl git build-essential libbz2-dev libdb-dev \
    libreadline-dev libffi-dev libgdbm-dev liblzma-dev \
    libncursesw5-dev libsqlite3-dev libssl-dev \
    zlib1g-dev uuid-dev tk-dev

ENV PYENV_ROOT $HOME/.pyenv
ENV PATH $PYENV_ROOT/bin:$PATH

RUN curl https://pyenv.run | bash && \
    echo '' >> /workspace/.bashrc && \
    echo 'export PYENV_ROOT="$HOME/.pyenv"' >> /workspace/.bashrc && \
    echo 'export PATH="$PYENV_ROOT/bin:$PATH"' >> /workspace/.bashrc && \
    echo 'eval "$(pyenv init --path)"' >> /workspace/.bashrc && \
    echo 'eval "$(pyenv virtualenv-init -)"' >> /workspace/.bashrc
RUN . /workspace/.bashrc && \
    PYTHON_CONFIGURE_OPTS="--enable-shared" pyenv install 3.10.12 && \
    pyenv global 3.10.12 && \
    pip install -U pip

RUN apt-get -y install wget unzip libboost-all-dev cmake

RUN mkdir /opt/protobuf &&\
    cd /opt/protobuf &&\
    wget https://github.com/protocolbuffers/protobuf/releases/download/v3.17.3/protobuf-cpp-3.17.3.tar.gz &&\
    tar zxvf protobuf-cpp-3.17.3.tar.gz &&\
    cd protobuf-3.17.3 &&\
    ./configure &&\
    make -j9 &&\
    make install &&\
    rm -rf /opt/protobuf &&\
    ldconfig

RUN adduser --disabled-password --gecos "" --home /workspace ubuntu &&\
    echo "ubuntu:ubuntu" | chpasswd &&\
    chown ubuntu:ubuntu /workspace
USER ubuntu

ENV HOME /workspace
ENV RUST_HOME ${HOME}/.local/lib/rust
ENV RUSTUP_HOME ${RUST_HOME}/rustup
ENV CARGO_HOME ${RUST_HOME}/cargo
RUN mkdir -p ${RUST_HOME} && \
    chmod 0755 $RUST_HOME
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs > ${RUST_HOME}/rustup.sh \
    && chmod +x ${RUST_HOME}/rustup.sh \
    && ${RUST_HOME}/rustup.sh -y --default-toolchain nightly --no-modify-path
ENV PATH $PATH:$CARGO_HOME/bin

COPY requirements.txt /workspace/
RUN . /workspace/.bashrc && python -m pip install -r requirements.txt
RUN . /workspace/.bashrc && python -m pip install git+https://github.com/smly/mjai.app.git  # 2023-08-18