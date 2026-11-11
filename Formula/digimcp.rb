class Digimcp < Formula
  desc "DigiiCampus student intelligence CLI and MCP server"
  homepage "https://github.com/kushvinth/DigiMCP"
  version "0.1.0"
  license "MIT"

  on_macos do
    url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.0/digimcp-macos-universal.tar.gz"
    sha256 "615241de0a9a83bc131ececd65825881d88163199d45982e319f229614209c08"
  end

  on_linux do
    on_arm do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.0/digimcp-linux-aarch64.tar.gz"
      sha256 "11d9aed832ba12ab0ea87dd409e11fe0cbdde3b1e7bd0511eac066e32be292e8"
    end

    on_intel do
      url "https://github.com/kushvinth/DigiMCP/releases/download/v0.1.0/digimcp-linux-x86_64.tar.gz"
      sha256 "71eef1304d6ebb369f1cab16529022ce2c53e06047d6e4a25815afa8bd85f4a0"
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
