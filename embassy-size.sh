#!/usr/bin/env bash

echo -e "commit,date,text,bss,dec,bin" > target/sizes.csv

set -e

for commit in $(git rev-list master)
do
    if [[ -f "examples/embassy-stm32/Cargo.toml" ]]; then
        pushd examples/embassy-stm32

        date=$(git show -s --format=%ci $commit)

        echo "Commit ${commit} at ${date}"

        git checkout $commit --quiet

        out=$(cargo size --release --quiet | tail -n1)
        text=$(echo $out | awk '{print $1}')
        bss=$(echo $out | awk '{print $3}')
        dec=$(echo $out | awk '{print $4}')

        cargo objcopy --release --quiet -- -O binary target/size.bin
        out=$(wc -c target/size.bin)
        bin=$(echo $out | awk '{print $1}')

        popd

        echo -e "$commit,$date,$text,$bss,$dec,$bin" >> target/sizes.csv
    fi
done

echo "Done"
