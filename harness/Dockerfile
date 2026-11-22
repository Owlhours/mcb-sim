# The headless Java harness in a container: JDK 25 plus the vanilla 26.2 server jar, downloaded from Mojang at
# image build time and checked against a pinned SHA-1 by harness/build.sh. Nothing from Mojang is in this repository,
# and nothing is installed beyond the base image.
#
#   docker build -f harness/Dockerfile -t mcb-harness .
#   docker run --rm -v "$PWD/schematics:/in:ro" mcb-harness build /in/house.litematic quiet
#   mkdir -p work
#   docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/schematics:/in:ro" -v "$PWD/work:/repo/work" mcb-harness prep /in/house.litematic
#   docker run --rm -u "$(id -u):$(id -g)" -v "$PWD/work:/repo/work" mcb-harness tables
FROM eclipse-temurin:25-jdk
WORKDIR /repo
ADD https://piston-data.mojang.com/v1/objects/823e2250d24b3ddac457a60c92a6a941943fcd6a/server.jar harness/.deps/server-26.2-bundler.jar
COPY harness/ harness/
RUN harness/build.sh && rm -f harness/.deps/server-26.2-bundler.jar && mkdir -p work && chmod 1777 work
ENTRYPOINT ["harness/run.sh"]
CMD ["help"]
