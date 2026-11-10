class Quad < Formula
  desc "DigiiCampus student intelligence CLI and MCP server"
  homepage "https://github.com/kushvinth/quad"
  version "0.1.0"
  license "MIT"

  if OS.mac?
    url "https://github.com/kushvinth/quad/releases/download/v0.1.0/quad-macos-universal.tar.gz"
  elsif OS.linux?
    if Hardware::CPU.arm?
      url "https://github.com/kushvinth/quad/releases/download/v0.1.0/quad-linux-aarch64.tar.gz"
    else
      url "https://github.com/kushvinth/quad/releases/download/v0.1.0/quad-linux-x86_64.tar.gz"
    end
  end

  def install
    bin.install "quad"
  end

  test do
    assert_match "quad", shell_output("#{bin}/quad --version")
  end
end
