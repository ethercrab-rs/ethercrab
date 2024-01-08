#!/usr/bin/env bash

echo -e "commit,date,loc_total,loc_code,loc_comments,text,bss,dec,bin,obj" > target/sizes.csv

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

        obj_size=$(cargo objcopy --release --quiet -- -O binary target/size.bin && wc -c target/size.bin | awk '{print $1}')

        popd
    fi

    code_stats=$(tokei --type Rust | tail -n2 | head -n1 | awk '{print $3","$4","$5}')

    echo -e "$commit,$date,$code_stats,$text,$bss,$dec,$bin,$obj_size" >> target/sizes.csv
done

echo "Done"
