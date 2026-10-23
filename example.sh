#!/bin/bash

wsm="./tars2sqlite4wasi.wasm"

hdir="$PWD/sample.d"
gdir="/guest.d"
bname="example.sqlite.db3"
gpath="${gdir}/${bname}"
hpath="${hdir}/${bname}"

max_entry_size=16777216
buffer_capacity4bufrdr=1048576
buffer_size=1048576

tar1="./sample.d/tar1.tar"
tar2="./sample.d/tar2.tar"
gz1="./sample.d/gz3.tar.gz"

geninput(){
  echo creating input files...

  mkdir -p ./sample.d

  echo t1f1 > ./sample.d/t1f1.txt
  echo t1f2 > ./sample.d/t1f2.txt

  echo t2f1 > ./sample.d/t2f1.txt

  echo g1f1 > ./sample.d/g1f1.txt

  ls ./sample.d/t1* | tar --files-from=- -c -v -f "${tar1}"
  ls ./sample.d/t2* | tar --files-from=- -c -v -f "${tar2}"
  ls ./sample.d/g1* | tar c --files-from=-| gzip --fast > "${gz1}"
}

run_wasi(){
  cat /dev/stdin |
    wasmtime \
      run \
      --env ENV_MAX_ENTRY_SIZE=$max_entry_size \
      --env ENV_BUFCAP4BUF_READER=$buffer_capacity4bufrdr \
      --env ENV_BUF_SIZE=$buffer_size \
      --env ENV_SQLITE_DB_FILE_PATH="${gpath}" \
      --dir "${hdir}::${gdir}" \
      "${wsm}"
}

test -f "${tar1}" || geninput
test -f "${tar2}" || geninput
test -f "${gz1}" || geninput

test -f "${hpath}" && echo "DROP TABLE IF EXISTS tars" | sqlite3 "${hpath}"
test -f "${hpath}" && echo "DROP TABLE IF EXISTS entries" | sqlite3 "${hpath}"

echo converting...
(
  ls "${tar1}"
  ls "${tar2}"
  ls "${gz1}"
) |
  cut -d/ -f3- |
  sed 's,^,/guest.d/,' |
  run_wasi
echo

echo 'printing tars table...'
echo "
  SELECT
    *
  FROM tars
" | sqlite3 -markdown "${hpath}"
echo

echo 'printing entries table...'
echo "
  SELECT
    *
  FROM entries
" | sqlite3 -markdown "${hpath}"
echo
