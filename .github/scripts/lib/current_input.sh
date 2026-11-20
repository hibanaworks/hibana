# Use the adjacent development source. The public revision bootstraps an
# absent checkout on CI and never rewinds local work.
checkout_current_input() {
    local directory="$1" repository="$2" revision="$3" evidence="$4"
    local name="${directory##*/}" actual
    if [[ ! -d "$directory" ]]; then
        git clone --quiet --filter=blob:none --no-checkout "$repository" "$directory"
        git -C "$directory" checkout --quiet --detach "$revision"
    fi
    actual="$(git -C "$directory" rev-parse HEAD)"
    printf '%s %s %s\n' "$name" "$actual" "$repository" >> "$evidence/input-revisions.txt"
    git -C "$directory" diff HEAD --binary > "$evidence/$name.patch"
}
