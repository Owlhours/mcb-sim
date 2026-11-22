@echo off
rem Runs one build of a schematic in the current folder with the mcb-harness Docker image (Docker Desktop).
rem   docker build -f harness\Dockerfile -t mcb-harness .      (once, from the repository root)
rem   harness\run-docker.bat house.litematic [quiet]
rem Untested on Windows; the same command works from WSL or any shell with Docker.
if "%~1"=="" ( echo usage: run-docker.bat ^<schematic in this folder^> [quiet] & exit /b 2 )
docker run --rm -v "%CD%:/in:ro" mcb-harness build "/in/%~nx1" %2
