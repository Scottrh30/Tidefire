#!/usr/bin/env bash
# Builds a small, static ffmpeg for Tidefire's video export: raw RGBA in on a
# pipe, H.264 (x264) in .mp4 or Apple ProRes in .mov out. Nothing else is
# compiled in, so the binary is a few MB and can be embedded in Tidefire.exe.
#
#   ci/build-ffmpeg.sh windows OUT_DIR   cross-compiles ffmpeg.exe with mingw-w64
#   ci/build-ffmpeg.sh native  OUT_DIR   same configuration for this machine (testing)
#
# Note: x264 is GPL, so this ffmpeg is a GPL build.
set -euo pipefail
TARGET=${1:?windows or native}
OUT=$(realpath -m "${2:?output dir}")
FFMPEG_TAG=n7.1
X264_COMMIT=c24e06c2e184345ceb33eb20a15d1024d9fd3497
WORK=${WORK:-$(mktemp -d)}
PREFIX=$WORK/prefix
mkdir -p "$OUT" "$PREFIX"
JOBS=$(nproc)

if [ "$TARGET" = windows ]; then
  HOST=x86_64-w64-mingw32
  X264_CROSS=(--host=$HOST --cross-prefix=$HOST-)
  FF_CROSS=(--target-os=mingw32 --arch=x86_64 --cross-prefix=$HOST-)
  EXE=ffmpeg.exe
  LDFLAGS="-static -static-libgcc"
else
  X264_CROSS=()
  FF_CROSS=()
  EXE=ffmpeg
  LDFLAGS=""
fi

cd "$WORK"
if [ ! -d x264 ]; then
  git clone --quiet https://github.com/mirror/x264 x264
  git -C x264 checkout --quiet $X264_COMMIT
fi
if [ ! -d ffmpeg ]; then
  git clone --quiet --depth 1 --branch $FFMPEG_TAG https://github.com/FFmpeg/FFmpeg ffmpeg
fi

(cd x264 && ./configure --prefix="$PREFIX" "${X264_CROSS[@]}" --enable-static --disable-cli --enable-pic \
  --disable-lavf --disable-swscale --disable-ffms --disable-gpac --disable-lsmash > /dev/null \
  && make -j"$JOBS" > /dev/null && make install > /dev/null)

cd ffmpeg
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" ./configure --prefix="$PREFIX" "${FF_CROSS[@]}" \
  --pkg-config=pkg-config --pkg-config-flags=--static \
  --enable-gpl --enable-libx264 \
  --disable-everything --disable-autodetect --disable-network --disable-doc \
  --disable-ffplay --disable-ffprobe --disable-debug \
  --enable-protocol=file,pipe \
  --enable-demuxer=rawvideo --enable-decoder=rawvideo \
  --enable-encoder=libx264,prores_ks \
  --enable-muxer=mp4,mov \
  --enable-filter=scale,pad,format,null,copy \
  --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib $LDFLAGS" > /dev/null
make -j"$JOBS" > /dev/null
cp ffmpeg$([ "$TARGET" = windows ] && echo .exe) "$OUT/$EXE"
${STRIP:-$([ "$TARGET" = windows ] && echo x86_64-w64-mingw32-strip || echo strip)} "$OUT/$EXE"
ls -la "$OUT/$EXE"
