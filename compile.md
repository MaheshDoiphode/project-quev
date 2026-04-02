Use this balanced setup for faster repeat builds (and still safer than defaults on 16 GB RAM):

Set-Location C:\Users\Mahes\Desktop\Projects\quev\zed
Remove-Item Env:CARGO_PROFILE_DEV_CODEGEN_UNITS,Env:CARGO_PROFILE_DEV_BUILD_OVERRIDE_CODEGEN_UNITS,Env:CARGO_PROFILE_DEV_BUILD_OVERRIDE_DEBUG -ErrorAction SilentlyContinue
$env:CARGO_INCREMENTAL='1'
$env:CARGO_BUILD_JOBS='2'
$env:CARGO_PROFILE_DEV_DEBUG='0'
cargo run -p zed -j 2



If OOM comes back, fall back to safer mode:

$env:CARGO_BUILD_JOBS='1'
$env:CARGO_INCREMENTAL='0'
$env:CARGO_PROFILE_DEV_CODEGEN_UNITS='1'
$env:CARGO_PROFILE_DEV_BUILD_OVERRIDE_CODEGEN_UNITS='1'
cargo run -p zed -j 1