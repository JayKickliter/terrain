set -e

cargo build --release -p demmit

mkdir -p target/animate

for azimuth in $(seq 0 359);
do
    echo $(printf "%03d" $azimuth)
    ./target/release/demmit                 \
        render                              \
        --azimuth=$azimuth                  \
        --constrain=1024                    \
        --elevation=45                      \
        --depth=8                           \
        data/nasadem/1arcsecond/N36W113.hgt \
        target/animate/$(printf "%03d" $azimuth).jpg
done


(
    set +e

    pushd target/animate

    ffmpeg             \
    -y                 \
    -framerate 30      \
    -pattern_type glob \
    -i '*.jpg'         \
    -c:v libx264       \
    -pix_fmt gray      \
    animate.mp4
)

exit $?
