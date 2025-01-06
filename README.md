Install SurrealDB CLI:

https://surrealdb.com/docs/surrealdb/installation

curl -sSf https://install.surrealdb.com | sh

* Start the DB Server in docker:
docker run --rm -p 8678:8000 surrealdb/surrealdb:latest start --user root --pass root

Deploy on EC2 Instance:
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
& sudo yum install gcc openssl-devel
$ sudo yum install nodejs npm
$ sudo yum groupinstall -y "Development Tools"
