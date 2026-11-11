class Digimcp < Formula
  desc "DigiiCampus student intelligence CLI and MCP server"
  homepage "https://github.com/kushvinth/DigiMCP"
  version "0.1.1"
  license "MIT"

  on_macos do
    url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-macos-universal.tar.gz"
  end

  on_linux do
    on_arm do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-linux-aarch64.tar.gz"
    end

    on_intel do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.1/digimcp-linux-x86_64.tar.gz"
    end
  end

  def install
    bin.install "digi"
    bin.install_symlink bin/"digi" => "digii"
    bin.install_symlink bin/"digi" => "digimcp"
  end

  test do
    assert_match "digi", shell_output("#{bin}/digi --version")
    assert_match "digi", shell_output("#{bin}/digimcp --version")
  end
end
