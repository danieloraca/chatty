Install SurrealDB CLI:

https://surrealdb.com/docs/surrealdb/installation

curl -sSf https://install.surrealdb.com | sh

* Start the DB Server in docker:
docker run --rm -p 8678:8000 surrealdb/surrealdb:latest start --user root --pass root
