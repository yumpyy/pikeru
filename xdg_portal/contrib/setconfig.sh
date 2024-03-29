#!/bin/bash
# This is from install.sh and is just the part that configures your user to use pikeru.

findconf(){
	current_desktop=${XDG_CURRENT_DESKTOP,,}
	dirs=(
		"$HOME/.config"
		"/etc/xdg"
		"/etc"
		"$HOME/.local/share"
		"/usr/local/share"
		"/usr/share"
	)
	for dir in "${dirs[@]}"; do
		a="${dir}/xdg-desktop-portal/portals.conf"
		b="${dir}/xdg-desktop-portal/${current_desktop}-portals.conf"
		[[ -f "$a" ]] && echo "$a" && return 0
		[[ -f "$b" ]] && echo "$b" && return 0
	done
	return 1
}

origconf="$(findconf)"
xdir="$HOME/.config/xdg-desktop-portal"
portalfile="$xdir/portals.conf"
mkdir -p "$xdir"

if [[ -f "$portalfile" ]]; then
	mv "$portalfile" "${portalfile}.orig"
	origconf="${portalfile}.orig"
fi

if [[ ! -z "$origconf" ]]; then
	sed '/FileChooser/d' "$origconf" > "$portalfile"
	echo 'org.freedesktop.impl.portal.FileChooser=pikeru' >> "$portalfile"
else
cat << EOF > "$portalfile"
[preferred]
default=auto
org.freedesktop.impl.portal.FileChooser=pikeru
EOF
fi

[[ "$origconf" =~ orig$ ]] && how="renaming $origconf to $(basename $portalfile)" || how='deleting it'
echo -e "\nYour new xdg-desktop-portal config is $portalfile. You can revert by ${how}"

systemctl --user restart xdg-desktop-portal.service