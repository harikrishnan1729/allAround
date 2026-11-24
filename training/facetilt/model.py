"""TinyOrientNet: 3 conv layers, ~28.6k parameters, 32x32x1 -> 5 logits.

The architecture can only express "which way is up" for a coarse 32x32
grayscale blob: there is no embedding output, no landmark head, and the
receptive field / capacity are far too small for identity recognition.
"""
import torch
from torch import nn

from .labels import NUM_CLASSES


class TinyOrientNet(nn.Module):
    def __init__(self, c1=16, c2=32, c3=64, dropout=0.2, side=32):
        super().__init__()
        self._fc_side = side // 8

        def block(cin, cout):
            return nn.Sequential(
                nn.Conv2d(cin, cout, 3, padding=1, bias=False),
                nn.BatchNorm2d(cout),
                nn.ReLU(inplace=True),
                nn.MaxPool2d(2),
            )

        self.features = nn.Sequential(block(1, c1), block(c1, c2), block(c2, c3))  # 32->16->8->4
        self.dropout = nn.Dropout(dropout)
        self.fc = nn.Linear(c3 * self._fc_side * self._fc_side, NUM_CLASSES)

    def forward(self, x):
        x = self.features(x)
        x = torch.flatten(x, 1)
        return self.fc(self.dropout(x))


def fuse_for_export(model: TinyOrientNet) -> nn.Module:
    """Folds BatchNorm into the preceding conv (eval mode) for a lean ONNX graph."""
    from torch.nn.utils.fusion import fuse_conv_bn_eval

    model = model.eval()
    fused = TinyOrientNet(side=model._fc_side * 8)
    fused.load_state_dict(model.state_dict())
    fused.eval()
    for blk in fused.features:
        conv, bn = blk[0], blk[1]
        blk[0] = fuse_conv_bn_eval(conv, bn)
        blk[1] = nn.Identity()
    return fused


def count_params(model: nn.Module) -> int:
    return sum(p.numel() for p in model.parameters())
