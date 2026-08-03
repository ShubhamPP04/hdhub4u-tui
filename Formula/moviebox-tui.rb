class MovieboxTui < Formula
  VERSION = "0.1.7"
  MACOS_SHA256 = "a7ff0f876d7170531df514da366a24f339a229625e60941a1c18b3b2147b7efa"
  LINUX_X64_SHA256 = "22c25c1c93e1623b3fca77678412b1307bf2725301d8a9b1cfb877d3520a6c77"
  LINUX_ARM64_SHA256 = "a45aa3b1c408ea02e07f7930a53ee051c5b55b63d03d04ebab984ec857a2db97"

  desc "Stream movies, shows, anime, and live TV from your terminal"
  homepage "https://github.com/mesamirh/MovieBox-Tui"
  version VERSION
  license "MIT"

  on_macos do
    url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_macOS_Universal.tar.gz"
    sha256 MACOS_SHA256
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_Linux_arm64.tar.gz"
      sha256 LINUX_ARM64_SHA256
    else
      url "https://github.com/mesamirh/MovieBox-Tui/releases/download/v#{VERSION}/MovieBox_Linux_x64.tar.gz"
      sha256 LINUX_X64_SHA256
    end
  end

  def install
    bin.install "moviebox-tui"
  end

  test do
    system "#{bin}/moviebox-tui", "--version"
  end
end
