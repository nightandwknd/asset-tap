//! Canonical humanoid skeleton, named with [VRM 1.0 humanoid bones][spec].
//!
//! Every rig operation speaks these 52 joints; pack-specific bone names are a
//! lookup at the edges ([`BoneScheme`]). Nothing downstream sniffs strings.
//!
//! **Why VRM and not a pack's own names.** Quaternius renamed the Universal
//! Animation Library's rig in January 2026: it used Blender Rigify names
//! (`DEF-hips`, `DEF-spine.001`) and now uses the Unreal convention
//! (`pelvis`, `spine_01`), with the rest geometry unchanged to 0.000 m. Had
//! we adopted the pack's scheme we would have pinned to a naming convention
//! upstream then abandoned. VRM is a published spec with real implementations
//! (UniVRM, three-vrm, the Blender VRM addon, and Unity's `HumanBodyBones`
//! share the set), maps 1:1 onto all 52 of our joints, and gives every future
//! pack (Mixamo, UE5, Ready Player Me) a documented mapping to write against.
//!
//! Writing VRM names into `model.glb` also leaves the door open to emitting a
//! `VRMC_vrm.humanoid` block later: that extension is metadata pointing at
//! nodes, and ours would already be named correctly.
//!
//! We use 52 of VRM's 55 bones, omitting `leftEye` / `rightEye` / `jaw` —
//! all optional, and absent from both packs.
//!
//! [spec]: https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/humanoid.md

use crate::rig::skeleton::{Armature, Joint};
use glam::{Quat, Vec3};

/// Canonical joints. Excludes the armature wrappers ([`ARMATURE_NAME`],
/// [`ROOT_NAME`]), which are structural rather than humanoid.
pub const JOINT_COUNT: usize = 52;

/// Provenance id for the embedded canonical skeleton. Distinct from a clip
/// pack id: fitting no longer depends on a pack being installed.
pub const SKELETON_ID: &str = "atap-humanoid-1";

/// glTF node name for the armature wrapper.
pub const ARMATURE_NAME: &str = "Rig";

/// glTF node name for the skeleton root. Not a humanoid bone: it exists so
/// `skeleton::fit` can scale and translate the whole rest pose,
/// and it is skin joint 0.
pub const ROOT_NAME: &str = "root";

/// The root's rest rotation — Blender's Z-up to glTF Y-up quarter turn. Baked
/// in so a canonical skeleton built from nothing matches a pack-derived one.
pub const ROOT_ROTATION: [f32; 4] = [-0.70710677, 0.0, 0.0, 0.70710677];

/// A canonical humanoid joint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HumanBone {
    /// `hips`
    Hips,
    /// `spine`
    Spine,
    /// `chest`
    Chest,
    /// `upperChest`
    UpperChest,
    /// `neck`
    Neck,
    /// `head`
    Head,
    /// `leftShoulder`
    LeftShoulder,
    /// `leftUpperArm`
    LeftUpperArm,
    /// `leftLowerArm`
    LeftLowerArm,
    /// `leftHand`
    LeftHand,
    /// `leftIndexProximal`
    LeftIndexProximal,
    /// `leftIndexIntermediate`
    LeftIndexIntermediate,
    /// `leftIndexDistal`
    LeftIndexDistal,
    /// `leftMiddleProximal`
    LeftMiddleProximal,
    /// `leftMiddleIntermediate`
    LeftMiddleIntermediate,
    /// `leftMiddleDistal`
    LeftMiddleDistal,
    /// `leftLittleProximal`
    LeftLittleProximal,
    /// `leftLittleIntermediate`
    LeftLittleIntermediate,
    /// `leftLittleDistal`
    LeftLittleDistal,
    /// `leftRingProximal`
    LeftRingProximal,
    /// `leftRingIntermediate`
    LeftRingIntermediate,
    /// `leftRingDistal`
    LeftRingDistal,
    /// `leftThumbMetacarpal`
    LeftThumbMetacarpal,
    /// `leftThumbProximal`
    LeftThumbProximal,
    /// `leftThumbDistal`
    LeftThumbDistal,
    /// `rightShoulder`
    RightShoulder,
    /// `rightUpperArm`
    RightUpperArm,
    /// `rightLowerArm`
    RightLowerArm,
    /// `rightHand`
    RightHand,
    /// `rightIndexProximal`
    RightIndexProximal,
    /// `rightIndexIntermediate`
    RightIndexIntermediate,
    /// `rightIndexDistal`
    RightIndexDistal,
    /// `rightMiddleProximal`
    RightMiddleProximal,
    /// `rightMiddleIntermediate`
    RightMiddleIntermediate,
    /// `rightMiddleDistal`
    RightMiddleDistal,
    /// `rightLittleProximal`
    RightLittleProximal,
    /// `rightLittleIntermediate`
    RightLittleIntermediate,
    /// `rightLittleDistal`
    RightLittleDistal,
    /// `rightRingProximal`
    RightRingProximal,
    /// `rightRingIntermediate`
    RightRingIntermediate,
    /// `rightRingDistal`
    RightRingDistal,
    /// `rightThumbMetacarpal`
    RightThumbMetacarpal,
    /// `rightThumbProximal`
    RightThumbProximal,
    /// `rightThumbDistal`
    RightThumbDistal,
    /// `leftUpperLeg`
    LeftUpperLeg,
    /// `leftLowerLeg`
    LeftLowerLeg,
    /// `leftFoot`
    LeftFoot,
    /// `leftToes`
    LeftToes,
    /// `rightUpperLeg`
    RightUpperLeg,
    /// `rightLowerLeg`
    RightLowerLeg,
    /// `rightFoot`
    RightFoot,
    /// `rightToes`
    RightToes,
}

/// What a joint's *head* sits on anatomically. The marker is drawn at the
/// head, so this is what the author is actually grabbing — and it is the
/// color key for the Rig wizard's legend.
///
/// Note the split between [`BoneGroup::Hips`] (the pelvis itself) and
/// [`BoneGroup::Hip`] (where each leg pivots), and between
/// [`BoneGroup::Clavicle`] and [`BoneGroup::Shoulder`]: those are adjacent
/// markers an author must be able to tell apart. Legend labels are Mixamo
/// nouns (`Clavicle`, `Shoulder`); VRM `leftShoulder` is the clavicle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BoneGroup {
    Hips,
    Spine,
    Neck,
    Head,
    Clavicle,
    Shoulder,
    Elbow,
    Wrist,
    Hip,
    Knee,
    Ankle,
    Toe,
    Finger,
}

impl BoneGroup {
    /// Legend label. Paired sides are labeled per marker, not here.
    pub const fn label(self) -> &'static str {
        match self {
            BoneGroup::Hips => "Pelvis",
            BoneGroup::Spine => "Spine",
            BoneGroup::Neck => "Neck",
            BoneGroup::Head => "Head",
            BoneGroup::Clavicle => "Clavicle",
            BoneGroup::Shoulder => "Shoulder",
            BoneGroup::Elbow => "Elbow",
            BoneGroup::Wrist => "Wrist",
            BoneGroup::Hip => "Hip",
            BoneGroup::Knee => "Knee",
            BoneGroup::Ankle => "Ankle",
            BoneGroup::Toe => "Toe",
            BoneGroup::Finger => "Finger",
        }
    }
}

/// Which of a paired joint this is. Drives the L/R marker label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const fn label(self) -> &'static str {
        match self {
            Side::Left => "L",
            Side::Right => "R",
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            Side::Left => "Left",
            Side::Right => "Right",
        }
    }
}

/// One joint of the canonical rest pose, in parent-local TRS.
///
/// Derived from the Quaternius Universal Animation Library rest pose (CC0).
/// Embedding it is what lets Rig and Bind run with no clip pack installed —
/// packs supply clips, never the skeleton.
#[derive(Debug, Clone, Copy)]
pub struct RestJoint {
    pub bone: HumanBone,
    pub parent: Option<HumanBone>,
    pub t: [f32; 3],
    pub r: [f32; 4],
    pub s: [f32; 3],
}

impl RestJoint {
    pub fn translation(&self) -> Vec3 {
        Vec3::from_array(self.t)
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_array(self.r)
    }

    pub fn scale(&self) -> Vec3 {
        Vec3::from_array(self.s)
    }
}

/// The canonical rest pose, parent-first. Indexed by `bone as usize`
/// (`rest_is_indexed_by_discriminant` holds that invariant).
pub const REST: [RestJoint; JOINT_COUNT] = [
    RestJoint {
        bone: HumanBone::Hips,
        parent: None,
        t: [0.0, 0.05010003, 0.9167],
        r: [0.7904685, 0.0, 0.0, 0.61250275],
        s: [1.0, 0.99999994, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::Spine,
        parent: Some(HumanBone::Hips),
        t: [0.0, 0.13817634, -1.21071935e-08],
        r: [-0.06470262, 0.0, 0.0, 0.9979046],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::Chest,
        parent: Some(HumanBone::Spine),
        t: [0.0, 0.12403485, 3.4924597e-10],
        r: [-0.07727984, 0.0, 0.0, 0.99700946],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::UpperChest,
        parent: Some(HumanBone::Chest),
        t: [0.0, 0.14127187, 5.5879354e-09],
        r: [-0.00026859157, 0.0, 0.0, 1.0],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::Neck,
        parent: Some(HumanBone::UpperChest),
        t: [0.0, 0.17289078, -1.8626451e-09],
        r: [0.11098591, 0.0, 0.0, 0.993822],
        s: [1.0, 0.99999994, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::Head,
        parent: Some(HumanBone::Neck),
        t: [0.0, 0.08258678, -1.3969839e-09],
        r: [-0.07867423, 0.0, 0.0, 0.9969004],
        s: [1.0, 0.99999994, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::LeftShoulder,
        parent: Some(HumanBone::UpperChest),
        t: [0.0188, 0.14055358, 0.08089504],
        r: [-0.6040206, -0.34510303, -0.35671768, 0.6235508],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftUpperArm,
        parent: Some(HumanBone::LeftShoulder),
        t: [-0.030072337, 0.21858175, -0.016968455],
        r: [0.18026958, 0.68385005, -0.1798364, 0.6837477],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftLowerArm,
        parent: Some(HumanBone::LeftUpperArm),
        t: [2.1565143e-08, 0.27444026, -1.2878445e-09],
        r: [0.01718229, -2.0353053e-05, 3.6708832e-07, 0.9998524],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftHand,
        parent: Some(HumanBone::LeftLowerArm),
        t: [1.2729826e-08, 0.27264056, -1.8102071e-09],
        r: [-0.008619683, -4.1722387e-07, 2.1919726e-09, 0.99996287],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftIndexProximal,
        parent: Some(HumanBone::LeftHand),
        t: [0.0019998318, 0.11989992, 0.030899998],
        r: [-4.8541157e-08, 0.7071096, -4.854154e-08, 0.70710397],
        s: [0.99999994, 0.99999976, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftIndexIntermediate,
        parent: Some(HumanBone::LeftIndexProximal),
        t: [-5.128199e-10, 0.04070002, 1.1360872e-08],
        r: [-4.4818607e-14, -9.671453e-09, -2.700417e-10, 1.0],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftIndexDistal,
        parent: Some(HumanBone::LeftIndexIntermediate),
        t: [9.722498e-10, 0.034800053, 1.0693611e-08],
        r: [7.348759e-18, 1.1368684e-13, -1.3279149e-16, 1.0],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftMiddleProximal,
        parent: Some(HumanBone::LeftHand),
        t: [-0.000400226, 0.121599905, 0.0052000005],
        r: [-0.01670246, 0.7069123, -0.01670257, 0.70690674],
        s: [0.99999994, 0.9999999, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftMiddleIntermediate,
        parent: Some(HumanBone::LeftMiddleProximal),
        t: [3.6379788e-10, 0.04234725, 2.0186121e-08],
        r: [-1.1900032e-08, -9.910035e-09, 0.0015135602, 0.99999887],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftMiddleDistal,
        parent: Some(HumanBone::LeftMiddleIntermediate),
        t: [-2.7830538e-10, 0.033933148, 1.6051382e-07],
        r: [8.156294e-09, -3.5669271e-12, -0.0011296597, 0.99999934],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftLittleProximal,
        parent: Some(HumanBone::LeftHand),
        t: [0.0015997173, 0.107699916, -0.0408],
        r: [-0.02370604, 0.70671207, -0.0237062, 0.7067065],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftLittleIntermediate,
        parent: Some(HumanBone::LeftLittleProximal),
        t: [3.3469405e-09, 0.04029057, -1.4873649e-09],
        r: [5.882123e-09, -1.0178269e-08, -0.0008342716, 0.99999964],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftLittleDistal,
        parent: Some(HumanBone::LeftLittleIntermediate),
        t: [-3.5843186e-09, 0.027665334, 2.2544884e-07],
        r: [-4.4004613e-09, 4.1868744e-12, 0.00060952286, 0.9999998],
        s: [0.99999994, 0.9999999, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::LeftRingProximal,
        parent: Some(HumanBone::LeftHand),
        t: [-0.0001001912, 0.1190999, -0.017100004],
        r: [-0.012588736, 0.7069975, -0.01258882, 0.706992],
        s: [0.99999994, 0.99999976, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftRingIntermediate,
        parent: Some(HumanBone::LeftRingProximal),
        t: [-7.8307494e-10, 0.039324984, -1.0246477e-07],
        r: [-8.660337e-10, -9.627518e-09, 1.2190081e-05, 1.0],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftRingDistal,
        parent: Some(HumanBone::LeftRingIntermediate),
        t: [-6.1763785e-09, 0.030919554, -1.0190598e-07],
        r: [7.704945e-09, -2.098989e-12, -0.0010670634, 0.9999994],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftThumbMetacarpal,
        parent: Some(HumanBone::LeftHand),
        t: [0.022799829, 0.027299935, 0.033599995],
        r: [0.24741302, 0.9457945, 0.20344116, 0.053584054],
        s: [1.0, 1.0000001, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftThumbProximal,
        parent: Some(HumanBone::LeftThumbMetacarpal),
        t: [5.9044396e-09, 0.043029793, 4.284084e-08],
        r: [-0.0001357142, -7.410717e-05, 4.7934565e-05, 1.0],
        s: [1.0, 0.9999999, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::LeftThumbDistal,
        parent: Some(HumanBone::LeftThumbProximal),
        t: [-1.2301234e-07, 0.049077947, -2.514571e-08],
        r: [0.00024788463, 8.0919184e-05, -0.0005353353, 0.9999998],
        s: [0.9999999, 0.9999999, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::RightShoulder,
        parent: Some(HumanBone::UpperChest),
        t: [-0.0188, 0.14055358, 0.08089504],
        r: [-0.6040206, 0.34510303, 0.35671768, 0.6235508],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightUpperArm,
        parent: Some(HumanBone::RightShoulder),
        t: [0.030072337, 0.21858175, -0.016968455],
        r: [0.18026958, -0.68385005, 0.1798364, 0.6837477],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightLowerArm,
        parent: Some(HumanBone::RightUpperArm),
        t: [-1.4812335e-07, 0.27444023, -7.941708e-09],
        r: [0.017182305, 2.0353053e-05, -3.6919505e-07, 0.9998524],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightHand,
        parent: Some(HumanBone::RightLowerArm),
        t: [-2.2043341e-08, 0.27264047, 1.7249704e-09],
        r: [-0.008619683, 5.3642873e-07, -2.1920292e-09, 0.99996287],
        s: [1.0, 0.99999994, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::RightIndexProximal,
        parent: Some(HumanBone::RightHand),
        t: [-0.0019998278, 0.11989996, 0.0309],
        r: [-4.8541143e-08, -0.70710963, 4.8541533e-08, 0.7071039],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightIndexIntermediate,
        parent: Some(HumanBone::RightIndexProximal),
        t: [-5.3552527e-09, 0.040700108, -1.078484e-07],
        r: [-5.2394443e-14, -3.824084e-09, 2.7004843e-10, 1.0],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightIndexDistal,
        parent: Some(HumanBone::RightIndexIntermediate),
        t: [-4.6966306e-09, 0.034800023, -1.0758447e-07],
        r: [-1.1838302e-16, -1.1368684e-13, -4.1818745e-16, 1.0],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightMiddleProximal,
        parent: Some(HumanBone::RightHand),
        t: [0.0004002332, 0.121599935, 0.0052000023],
        r: [-0.01670246, -0.70691234, 0.016702566, 0.7069067],
        s: [0.99999994, 0.9999999, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightMiddleIntermediate,
        parent: Some(HumanBone::RightMiddleProximal),
        t: [6.311893e-10, 0.042347282, -9.737596e-08],
        r: [-1.1009926e-08, -3.383724e-09, -0.0015135611, 0.99999887],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightMiddleDistal,
        parent: Some(HumanBone::RightMiddleIntermediate),
        t: [2.14186e-09, 0.03393318, 4.1895845e-08],
        r: [8.186299e-09, 2.257529e-11, 0.0011296588, 0.99999934],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightLittleProximal,
        parent: Some(HumanBone::RightHand),
        t: [-0.0015997047, 0.107699946, -0.0408],
        r: [-0.02370604, -0.7067121, 0.023706198, 0.70670646],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightLittleIntermediate,
        parent: Some(HumanBone::RightLittleProximal),
        t: [8.576535e-10, 0.04029054, -1.963656e-09],
        r: [5.623981e-09, -2.9140144e-09, 0.00083426427, 0.99999964],
        s: [0.99999994, 0.9999998, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightLittleDistal,
        parent: Some(HumanBone::RightLittleIntermediate),
        t: [3.5843186e-09, 0.027665364, -1.1715912e-08],
        r: [-4.41642e-09, -1.6191496e-11, -0.00060952094, 0.9999998],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightRingProximal,
        parent: Some(HumanBone::RightHand),
        t: [0.00010020104, 0.11909994, -0.017100003],
        r: [-0.012588738, -0.7069975, 0.012588818, 0.706992],
        s: [0.99999994, 0.99999976, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightRingIntermediate,
        parent: Some(HumanBone::RightRingProximal),
        t: [-2.5420377e-09, 0.039325017, -2.1878903e-07],
        r: [7.371431e-10, -3.4386891e-09, -1.21910125e-05, 1.0],
        s: [1.0, 1.0, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightRingDistal,
        parent: Some(HumanBone::RightRingIntermediate),
        t: [2.4510882e-09, 0.030919585, -1.0054916e-07],
        r: [7.732829e-09, 2.1304748e-11, 0.0010670634, 0.9999994],
        s: [1.0, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightThumbMetacarpal,
        parent: Some(HumanBone::RightHand),
        t: [-0.022799823, 0.027299969, 0.033599995],
        r: [0.24741305, -0.94579446, -0.2034411, 0.053584058],
        s: [1.0, 1.0000001, 1.0],
    },
    RestJoint {
        bone: HumanBone::RightThumbProximal,
        parent: Some(HumanBone::RightThumbMetacarpal),
        t: [-7.172275e-09, 0.04302986, -1.6763806e-08],
        r: [-0.0001356695, 7.4015756e-05, -4.7864316e-05, 1.0],
        s: [1.0, 0.99999994, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::RightThumbDistal,
        parent: Some(HumanBone::RightThumbProximal),
        t: [-5.580432e-08, 0.049077883, 5.5879354e-09],
        r: [0.00024785104, -8.099936e-05, 0.00053531677, 0.9999999],
        s: [0.9999999, 0.99999994, 1.0],
    },
    RestJoint {
        bone: HumanBone::LeftUpperLeg,
        parent: Some(HumanBone::Hips),
        t: [0.089, 0.027770841, 0.046023797],
        r: [0.99248445, 0.0, 0.0, 0.12237062],
        s: [1.0, 1.0, 0.9999998],
    },
    RestJoint {
        bone: HumanBone::LeftLowerLeg,
        parent: Some(HumanBone::LeftUpperLeg),
        t: [7.450581e-09, 0.40030977, -2.3283064e-10],
        r: [0.03658591, -0.00013117322, -4.802307e-06, 0.9993305],
        s: [0.99999994, 0.9999999, 0.9999999],
    },
    RestJoint {
        bone: HumanBone::LeftFoot,
        parent: Some(HumanBone::LeftLowerLeg),
        t: [-7.219114e-09, 0.4294799, 2.408342e-09],
        r: [-0.52907175, -0.00032809316, 0.00034345294, 0.848577],
        s: [1.0, 1.0, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::LeftToes,
        parent: Some(HumanBone::LeftFoot),
        t: [3.919922e-09, 0.17330109, -8.578354e-09],
        r: [0.0001371279, -0.9643068, 0.2647871, 0.0004994344],
        s: [1.0, 1.0000004, 0.9999998],
    },
    RestJoint {
        bone: HumanBone::RightUpperLeg,
        parent: Some(HumanBone::Hips),
        t: [-0.089, 0.027770841, 0.046023797],
        r: [0.99248445, 0.0, 0.0, 0.12237062],
        s: [1.0, 1.0, 0.9999998],
    },
    RestJoint {
        bone: HumanBone::RightLowerLeg,
        parent: Some(HumanBone::RightUpperLeg),
        t: [-7.450581e-09, 0.40030977, -2.3283064e-10],
        r: [0.03658591, -0.00013117322, -4.802307e-06, 0.9993305],
        s: [0.99999994, 0.9999999, 0.9999999],
    },
    RestJoint {
        bone: HumanBone::RightFoot,
        parent: Some(HumanBone::RightLowerLeg),
        t: [7.682047e-09, 0.42947984, 1.3169483e-09],
        r: [-0.52907175, -0.00032809316, 0.00034345294, 0.848577],
        s: [1.0, 1.0, 0.99999994],
    },
    RestJoint {
        bone: HumanBone::RightToes,
        parent: Some(HumanBone::RightFoot),
        t: [-3.5306584e-09, 0.17330109, -8.185452e-09],
        r: [0.0001371279, -0.9643068, 0.2647871, 0.0004994344],
        s: [1.0, 1.0000004, 0.9999998],
    },
];

impl HumanBone {
    /// Parent-first, matching [`REST`].
    pub const ALL: [HumanBone; JOINT_COUNT] = [
        HumanBone::Hips,
        HumanBone::Spine,
        HumanBone::Chest,
        HumanBone::UpperChest,
        HumanBone::Neck,
        HumanBone::Head,
        HumanBone::LeftShoulder,
        HumanBone::LeftUpperArm,
        HumanBone::LeftLowerArm,
        HumanBone::LeftHand,
        HumanBone::LeftIndexProximal,
        HumanBone::LeftIndexIntermediate,
        HumanBone::LeftIndexDistal,
        HumanBone::LeftMiddleProximal,
        HumanBone::LeftMiddleIntermediate,
        HumanBone::LeftMiddleDistal,
        HumanBone::LeftLittleProximal,
        HumanBone::LeftLittleIntermediate,
        HumanBone::LeftLittleDistal,
        HumanBone::LeftRingProximal,
        HumanBone::LeftRingIntermediate,
        HumanBone::LeftRingDistal,
        HumanBone::LeftThumbMetacarpal,
        HumanBone::LeftThumbProximal,
        HumanBone::LeftThumbDistal,
        HumanBone::RightShoulder,
        HumanBone::RightUpperArm,
        HumanBone::RightLowerArm,
        HumanBone::RightHand,
        HumanBone::RightIndexProximal,
        HumanBone::RightIndexIntermediate,
        HumanBone::RightIndexDistal,
        HumanBone::RightMiddleProximal,
        HumanBone::RightMiddleIntermediate,
        HumanBone::RightMiddleDistal,
        HumanBone::RightLittleProximal,
        HumanBone::RightLittleIntermediate,
        HumanBone::RightLittleDistal,
        HumanBone::RightRingProximal,
        HumanBone::RightRingIntermediate,
        HumanBone::RightRingDistal,
        HumanBone::RightThumbMetacarpal,
        HumanBone::RightThumbProximal,
        HumanBone::RightThumbDistal,
        HumanBone::LeftUpperLeg,
        HumanBone::LeftLowerLeg,
        HumanBone::LeftFoot,
        HumanBone::LeftToes,
        HumanBone::RightUpperLeg,
        HumanBone::RightLowerLeg,
        HumanBone::RightFoot,
        HumanBone::RightToes,
    ];

    /// The VRM 1.0 bone name. This is what we write into glTF.
    pub const fn as_str(self) -> &'static str {
        match self {
            HumanBone::Hips => "hips",
            HumanBone::Spine => "spine",
            HumanBone::Chest => "chest",
            HumanBone::UpperChest => "upperChest",
            HumanBone::Neck => "neck",
            HumanBone::Head => "head",
            HumanBone::LeftShoulder => "leftShoulder",
            HumanBone::LeftUpperArm => "leftUpperArm",
            HumanBone::LeftLowerArm => "leftLowerArm",
            HumanBone::LeftHand => "leftHand",
            HumanBone::LeftIndexProximal => "leftIndexProximal",
            HumanBone::LeftIndexIntermediate => "leftIndexIntermediate",
            HumanBone::LeftIndexDistal => "leftIndexDistal",
            HumanBone::LeftMiddleProximal => "leftMiddleProximal",
            HumanBone::LeftMiddleIntermediate => "leftMiddleIntermediate",
            HumanBone::LeftMiddleDistal => "leftMiddleDistal",
            HumanBone::LeftLittleProximal => "leftLittleProximal",
            HumanBone::LeftLittleIntermediate => "leftLittleIntermediate",
            HumanBone::LeftLittleDistal => "leftLittleDistal",
            HumanBone::LeftRingProximal => "leftRingProximal",
            HumanBone::LeftRingIntermediate => "leftRingIntermediate",
            HumanBone::LeftRingDistal => "leftRingDistal",
            HumanBone::LeftThumbMetacarpal => "leftThumbMetacarpal",
            HumanBone::LeftThumbProximal => "leftThumbProximal",
            HumanBone::LeftThumbDistal => "leftThumbDistal",
            HumanBone::RightShoulder => "rightShoulder",
            HumanBone::RightUpperArm => "rightUpperArm",
            HumanBone::RightLowerArm => "rightLowerArm",
            HumanBone::RightHand => "rightHand",
            HumanBone::RightIndexProximal => "rightIndexProximal",
            HumanBone::RightIndexIntermediate => "rightIndexIntermediate",
            HumanBone::RightIndexDistal => "rightIndexDistal",
            HumanBone::RightMiddleProximal => "rightMiddleProximal",
            HumanBone::RightMiddleIntermediate => "rightMiddleIntermediate",
            HumanBone::RightMiddleDistal => "rightMiddleDistal",
            HumanBone::RightLittleProximal => "rightLittleProximal",
            HumanBone::RightLittleIntermediate => "rightLittleIntermediate",
            HumanBone::RightLittleDistal => "rightLittleDistal",
            HumanBone::RightRingProximal => "rightRingProximal",
            HumanBone::RightRingIntermediate => "rightRingIntermediate",
            HumanBone::RightRingDistal => "rightRingDistal",
            HumanBone::RightThumbMetacarpal => "rightThumbMetacarpal",
            HumanBone::RightThumbProximal => "rightThumbProximal",
            HumanBone::RightThumbDistal => "rightThumbDistal",
            HumanBone::LeftUpperLeg => "leftUpperLeg",
            HumanBone::LeftLowerLeg => "leftLowerLeg",
            HumanBone::LeftFoot => "leftFoot",
            HumanBone::LeftToes => "leftToes",
            HumanBone::RightUpperLeg => "rightUpperLeg",
            HumanBone::RightLowerLeg => "rightLowerLeg",
            HumanBone::RightFoot => "rightFoot",
            HumanBone::RightToes => "rightToes",
        }
    }

    /// Parse a canonical name. Pack names go through
    /// [`BoneScheme::to_canonical`] instead.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "hips" => Some(HumanBone::Hips),
            "spine" => Some(HumanBone::Spine),
            "chest" => Some(HumanBone::Chest),
            "upperChest" => Some(HumanBone::UpperChest),
            "neck" => Some(HumanBone::Neck),
            "head" => Some(HumanBone::Head),
            "leftShoulder" => Some(HumanBone::LeftShoulder),
            "leftUpperArm" => Some(HumanBone::LeftUpperArm),
            "leftLowerArm" => Some(HumanBone::LeftLowerArm),
            "leftHand" => Some(HumanBone::LeftHand),
            "leftIndexProximal" => Some(HumanBone::LeftIndexProximal),
            "leftIndexIntermediate" => Some(HumanBone::LeftIndexIntermediate),
            "leftIndexDistal" => Some(HumanBone::LeftIndexDistal),
            "leftMiddleProximal" => Some(HumanBone::LeftMiddleProximal),
            "leftMiddleIntermediate" => Some(HumanBone::LeftMiddleIntermediate),
            "leftMiddleDistal" => Some(HumanBone::LeftMiddleDistal),
            "leftLittleProximal" => Some(HumanBone::LeftLittleProximal),
            "leftLittleIntermediate" => Some(HumanBone::LeftLittleIntermediate),
            "leftLittleDistal" => Some(HumanBone::LeftLittleDistal),
            "leftRingProximal" => Some(HumanBone::LeftRingProximal),
            "leftRingIntermediate" => Some(HumanBone::LeftRingIntermediate),
            "leftRingDistal" => Some(HumanBone::LeftRingDistal),
            "leftThumbMetacarpal" => Some(HumanBone::LeftThumbMetacarpal),
            "leftThumbProximal" => Some(HumanBone::LeftThumbProximal),
            "leftThumbDistal" => Some(HumanBone::LeftThumbDistal),
            "rightShoulder" => Some(HumanBone::RightShoulder),
            "rightUpperArm" => Some(HumanBone::RightUpperArm),
            "rightLowerArm" => Some(HumanBone::RightLowerArm),
            "rightHand" => Some(HumanBone::RightHand),
            "rightIndexProximal" => Some(HumanBone::RightIndexProximal),
            "rightIndexIntermediate" => Some(HumanBone::RightIndexIntermediate),
            "rightIndexDistal" => Some(HumanBone::RightIndexDistal),
            "rightMiddleProximal" => Some(HumanBone::RightMiddleProximal),
            "rightMiddleIntermediate" => Some(HumanBone::RightMiddleIntermediate),
            "rightMiddleDistal" => Some(HumanBone::RightMiddleDistal),
            "rightLittleProximal" => Some(HumanBone::RightLittleProximal),
            "rightLittleIntermediate" => Some(HumanBone::RightLittleIntermediate),
            "rightLittleDistal" => Some(HumanBone::RightLittleDistal),
            "rightRingProximal" => Some(HumanBone::RightRingProximal),
            "rightRingIntermediate" => Some(HumanBone::RightRingIntermediate),
            "rightRingDistal" => Some(HumanBone::RightRingDistal),
            "rightThumbMetacarpal" => Some(HumanBone::RightThumbMetacarpal),
            "rightThumbProximal" => Some(HumanBone::RightThumbProximal),
            "rightThumbDistal" => Some(HumanBone::RightThumbDistal),
            "leftUpperLeg" => Some(HumanBone::LeftUpperLeg),
            "leftLowerLeg" => Some(HumanBone::LeftLowerLeg),
            "leftFoot" => Some(HumanBone::LeftFoot),
            "leftToes" => Some(HumanBone::LeftToes),
            "rightUpperLeg" => Some(HumanBone::RightUpperLeg),
            "rightLowerLeg" => Some(HumanBone::RightLowerLeg),
            "rightFoot" => Some(HumanBone::RightFoot),
            "rightToes" => Some(HumanBone::RightToes),
            _ => None,
        }
    }

    pub const fn group(self) -> BoneGroup {
        match self {
            HumanBone::Hips => BoneGroup::Hips,
            HumanBone::Spine => BoneGroup::Spine,
            HumanBone::Chest => BoneGroup::Spine,
            HumanBone::UpperChest => BoneGroup::Spine,
            HumanBone::Neck => BoneGroup::Neck,
            HumanBone::Head => BoneGroup::Head,
            HumanBone::LeftShoulder => BoneGroup::Clavicle,
            HumanBone::LeftUpperArm => BoneGroup::Shoulder,
            HumanBone::LeftLowerArm => BoneGroup::Elbow,
            HumanBone::LeftHand => BoneGroup::Wrist,
            HumanBone::LeftIndexProximal => BoneGroup::Finger,
            HumanBone::LeftIndexIntermediate => BoneGroup::Finger,
            HumanBone::LeftIndexDistal => BoneGroup::Finger,
            HumanBone::LeftMiddleProximal => BoneGroup::Finger,
            HumanBone::LeftMiddleIntermediate => BoneGroup::Finger,
            HumanBone::LeftMiddleDistal => BoneGroup::Finger,
            HumanBone::LeftLittleProximal => BoneGroup::Finger,
            HumanBone::LeftLittleIntermediate => BoneGroup::Finger,
            HumanBone::LeftLittleDistal => BoneGroup::Finger,
            HumanBone::LeftRingProximal => BoneGroup::Finger,
            HumanBone::LeftRingIntermediate => BoneGroup::Finger,
            HumanBone::LeftRingDistal => BoneGroup::Finger,
            HumanBone::LeftThumbMetacarpal => BoneGroup::Finger,
            HumanBone::LeftThumbProximal => BoneGroup::Finger,
            HumanBone::LeftThumbDistal => BoneGroup::Finger,
            HumanBone::RightShoulder => BoneGroup::Clavicle,
            HumanBone::RightUpperArm => BoneGroup::Shoulder,
            HumanBone::RightLowerArm => BoneGroup::Elbow,
            HumanBone::RightHand => BoneGroup::Wrist,
            HumanBone::RightIndexProximal => BoneGroup::Finger,
            HumanBone::RightIndexIntermediate => BoneGroup::Finger,
            HumanBone::RightIndexDistal => BoneGroup::Finger,
            HumanBone::RightMiddleProximal => BoneGroup::Finger,
            HumanBone::RightMiddleIntermediate => BoneGroup::Finger,
            HumanBone::RightMiddleDistal => BoneGroup::Finger,
            HumanBone::RightLittleProximal => BoneGroup::Finger,
            HumanBone::RightLittleIntermediate => BoneGroup::Finger,
            HumanBone::RightLittleDistal => BoneGroup::Finger,
            HumanBone::RightRingProximal => BoneGroup::Finger,
            HumanBone::RightRingIntermediate => BoneGroup::Finger,
            HumanBone::RightRingDistal => BoneGroup::Finger,
            HumanBone::RightThumbMetacarpal => BoneGroup::Finger,
            HumanBone::RightThumbProximal => BoneGroup::Finger,
            HumanBone::RightThumbDistal => BoneGroup::Finger,
            HumanBone::LeftUpperLeg => BoneGroup::Hip,
            HumanBone::LeftLowerLeg => BoneGroup::Knee,
            HumanBone::LeftFoot => BoneGroup::Ankle,
            HumanBone::LeftToes => BoneGroup::Toe,
            HumanBone::RightUpperLeg => BoneGroup::Hip,
            HumanBone::RightLowerLeg => BoneGroup::Knee,
            HumanBone::RightFoot => BoneGroup::Ankle,
            HumanBone::RightToes => BoneGroup::Toe,
        }
    }

    pub const fn side(self) -> Option<Side> {
        match self {
            HumanBone::Hips => None,
            HumanBone::Spine => None,
            HumanBone::Chest => None,
            HumanBone::UpperChest => None,
            HumanBone::Neck => None,
            HumanBone::Head => None,
            HumanBone::LeftShoulder => Some(Side::Left),
            HumanBone::LeftUpperArm => Some(Side::Left),
            HumanBone::LeftLowerArm => Some(Side::Left),
            HumanBone::LeftHand => Some(Side::Left),
            HumanBone::LeftIndexProximal => Some(Side::Left),
            HumanBone::LeftIndexIntermediate => Some(Side::Left),
            HumanBone::LeftIndexDistal => Some(Side::Left),
            HumanBone::LeftMiddleProximal => Some(Side::Left),
            HumanBone::LeftMiddleIntermediate => Some(Side::Left),
            HumanBone::LeftMiddleDistal => Some(Side::Left),
            HumanBone::LeftLittleProximal => Some(Side::Left),
            HumanBone::LeftLittleIntermediate => Some(Side::Left),
            HumanBone::LeftLittleDistal => Some(Side::Left),
            HumanBone::LeftRingProximal => Some(Side::Left),
            HumanBone::LeftRingIntermediate => Some(Side::Left),
            HumanBone::LeftRingDistal => Some(Side::Left),
            HumanBone::LeftThumbMetacarpal => Some(Side::Left),
            HumanBone::LeftThumbProximal => Some(Side::Left),
            HumanBone::LeftThumbDistal => Some(Side::Left),
            HumanBone::RightShoulder => Some(Side::Right),
            HumanBone::RightUpperArm => Some(Side::Right),
            HumanBone::RightLowerArm => Some(Side::Right),
            HumanBone::RightHand => Some(Side::Right),
            HumanBone::RightIndexProximal => Some(Side::Right),
            HumanBone::RightIndexIntermediate => Some(Side::Right),
            HumanBone::RightIndexDistal => Some(Side::Right),
            HumanBone::RightMiddleProximal => Some(Side::Right),
            HumanBone::RightMiddleIntermediate => Some(Side::Right),
            HumanBone::RightMiddleDistal => Some(Side::Right),
            HumanBone::RightLittleProximal => Some(Side::Right),
            HumanBone::RightLittleIntermediate => Some(Side::Right),
            HumanBone::RightLittleDistal => Some(Side::Right),
            HumanBone::RightRingProximal => Some(Side::Right),
            HumanBone::RightRingIntermediate => Some(Side::Right),
            HumanBone::RightRingDistal => Some(Side::Right),
            HumanBone::RightThumbMetacarpal => Some(Side::Right),
            HumanBone::RightThumbProximal => Some(Side::Right),
            HumanBone::RightThumbDistal => Some(Side::Right),
            HumanBone::LeftUpperLeg => Some(Side::Left),
            HumanBone::LeftLowerLeg => Some(Side::Left),
            HumanBone::LeftFoot => Some(Side::Left),
            HumanBone::LeftToes => Some(Side::Left),
            HumanBone::RightUpperLeg => Some(Side::Right),
            HumanBone::RightLowerLeg => Some(Side::Right),
            HumanBone::RightFoot => Some(Side::Right),
            HumanBone::RightToes => Some(Side::Right),
        }
    }

    /// Fingers stay in the exported skin (clips target them) but never
    /// receive weights: see `weights::is_bind_bone`.
    pub const fn is_finger(self) -> bool {
        matches!(self.group(), BoneGroup::Finger)
    }

    /// Joints the workbench may show, drag, or write.
    pub const fn is_placeable(self) -> bool {
        !self.is_finger()
    }

    /// Name shown when a marker is selected. Same vocabulary as the legend
    /// (`Clavicle`, `Shoulder`), not the VRM wire (`leftShoulder`, `leftUpperArm`).
    pub fn panel_label(self) -> String {
        match self.side() {
            Some(side) => format!(
                "{} {}",
                side.word(),
                self.group().label().to_ascii_lowercase()
            ),
            None => self.group().label().to_string(),
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn rest(self) -> &'static RestJoint {
        &REST[self as usize]
    }

    pub fn parent(self) -> Option<HumanBone> {
        self.rest().parent
    }

    /// This joint's name in `scheme`.
    pub const fn name_in(self, scheme: BoneScheme) -> &'static str {
        match scheme {
            BoneScheme::Vrm => self.as_str(),
            BoneScheme::Unreal => self.unreal_name(),
        }
    }

    /// Unreal skeleton convention — Quaternius UAL2.
    const fn unreal_name(self) -> &'static str {
        match self {
            HumanBone::Hips => "pelvis",
            HumanBone::Spine => "spine_01",
            HumanBone::Chest => "spine_02",
            HumanBone::UpperChest => "spine_03",
            HumanBone::Neck => "neck_01",
            HumanBone::Head => "Head",
            HumanBone::LeftShoulder => "clavicle_l",
            HumanBone::LeftUpperArm => "upperarm_l",
            HumanBone::LeftLowerArm => "lowerarm_l",
            HumanBone::LeftHand => "hand_l",
            HumanBone::LeftIndexProximal => "index_01_l",
            HumanBone::LeftIndexIntermediate => "index_02_l",
            HumanBone::LeftIndexDistal => "index_03_l",
            HumanBone::LeftMiddleProximal => "middle_01_l",
            HumanBone::LeftMiddleIntermediate => "middle_02_l",
            HumanBone::LeftMiddleDistal => "middle_03_l",
            HumanBone::LeftLittleProximal => "pinky_01_l",
            HumanBone::LeftLittleIntermediate => "pinky_02_l",
            HumanBone::LeftLittleDistal => "pinky_03_l",
            HumanBone::LeftRingProximal => "ring_01_l",
            HumanBone::LeftRingIntermediate => "ring_02_l",
            HumanBone::LeftRingDistal => "ring_03_l",
            HumanBone::LeftThumbMetacarpal => "thumb_01_l",
            HumanBone::LeftThumbProximal => "thumb_02_l",
            HumanBone::LeftThumbDistal => "thumb_03_l",
            HumanBone::RightShoulder => "clavicle_r",
            HumanBone::RightUpperArm => "upperarm_r",
            HumanBone::RightLowerArm => "lowerarm_r",
            HumanBone::RightHand => "hand_r",
            HumanBone::RightIndexProximal => "index_01_r",
            HumanBone::RightIndexIntermediate => "index_02_r",
            HumanBone::RightIndexDistal => "index_03_r",
            HumanBone::RightMiddleProximal => "middle_01_r",
            HumanBone::RightMiddleIntermediate => "middle_02_r",
            HumanBone::RightMiddleDistal => "middle_03_r",
            HumanBone::RightLittleProximal => "pinky_01_r",
            HumanBone::RightLittleIntermediate => "pinky_02_r",
            HumanBone::RightLittleDistal => "pinky_03_r",
            HumanBone::RightRingProximal => "ring_01_r",
            HumanBone::RightRingIntermediate => "ring_02_r",
            HumanBone::RightRingDistal => "ring_03_r",
            HumanBone::RightThumbMetacarpal => "thumb_01_r",
            HumanBone::RightThumbProximal => "thumb_02_r",
            HumanBone::RightThumbDistal => "thumb_03_r",
            HumanBone::LeftUpperLeg => "thigh_l",
            HumanBone::LeftLowerLeg => "calf_l",
            HumanBone::LeftFoot => "foot_l",
            HumanBone::LeftToes => "ball_l",
            HumanBone::RightUpperLeg => "thigh_r",
            HumanBone::RightLowerLeg => "calf_r",
            HumanBone::RightFoot => "foot_r",
            HumanBone::RightToes => "ball_r",
        }
    }
}

/// A bone-naming convention we can read.
///
/// Adding a pack family means adding a variant and its name table, never
/// touching fit, weighting, or retargeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoneScheme {
    /// Our own output, and anything else VRM-named.
    Vrm,
    /// Unreal-style lowercase with `_l` / `_r` — every current Quaternius
    /// pack (both UAL1 v2.0 and UAL2 use this since the January 2026 rename).
    Unreal,
}

impl BoneScheme {
    pub const ALL: [BoneScheme; 2] = [BoneScheme::Vrm, BoneScheme::Unreal];

    /// Map one bone name in this scheme to its canonical joint.
    pub fn to_canonical(self, name: &str) -> Option<HumanBone> {
        HumanBone::ALL.into_iter().find(|b| b.name_in(self) == name)
    }

    /// How many of `names` this scheme recognizes.
    pub fn score<'a>(self, names: impl IntoIterator<Item = &'a str>) -> usize {
        names
            .into_iter()
            .filter(|n| self.to_canonical(n).is_some())
            .count()
    }

    /// Best-matching scheme for a node-name set, or `None` when nothing
    /// resembles a humanoid.
    ///
    /// Scored rather than probed on a single bone: a pack that renames one
    /// joint should still resolve, and a mesh that happens to contain a node
    /// called `head` should not.
    pub fn detect<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let names: Vec<&str> = names.into_iter().collect();
        let (best, score) = BoneScheme::ALL
            .into_iter()
            .map(|s| (s, s.score(names.iter().copied())))
            .max_by_key(|&(_, n)| n)?;
        (score >= MIN_DETECT_MATCHES).then_some(best)
    }
}

/// Enough joints to be a humanoid rig rather than a coincidence. The major
/// deformers number 22, so half of them is a comfortable floor.
const MIN_DETECT_MATCHES: usize = 11;

/// Node index of [`ROOT_NAME`] in [`armature`].
const ROOT_NODE: usize = 1;
/// Node index of the first canonical joint in [`armature`].
const FIRST_JOINT_NODE: usize = 2;

/// The canonical rest armature: `Rig` → `root` → the 52 joints.
///
/// This replaces reading a skeleton out of a clip pack, which is what lets Rig
/// and Bind run with nothing installed — packs supply clips, never bones. The
/// wrapper nodes are kept because `skeleton::fit` scales and translates the
/// topmost node to size the whole rest pose, and `root` is skin joint 0.
pub(crate) fn armature() -> Armature {
    let mut joints = Vec::with_capacity(JOINT_COUNT + FIRST_JOINT_NODE);
    joints.push(Joint {
        name: ARMATURE_NAME.to_string(),
        parent: None,
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    });
    joints.push(Joint {
        name: ROOT_NAME.to_string(),
        parent: Some(0),
        translation: Vec3::ZERO,
        rotation: Quat::from_array(ROOT_ROTATION),
        scale: Vec3::ONE,
    });
    for joint in &REST {
        joints.push(Joint {
            name: joint.bone.as_str().to_string(),
            parent: Some(
                joint
                    .parent
                    .map_or(ROOT_NODE, |p| p.index() + FIRST_JOINT_NODE),
            ),
            translation: joint.translation(),
            rotation: joint.rotation(),
            scale: joint.scale(),
        });
    }
    let name_to_index = joints
        .iter()
        .enumerate()
        .map(|(i, j)| (j.name.clone(), i))
        .collect();
    let skin = std::iter::once(ROOT_NODE)
        .chain((0..JOINT_COUNT).map(|i| i + FIRST_JOINT_NODE))
        .collect();
    Armature {
        joints,
        skin,
        name_to_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// World heads of the Quaternius rest pose, measured from the pack GLB.
    /// The embedded table has to reproduce these or every clip retarget is
    /// computed against the wrong rest.
    const GOLDEN: &[(HumanBone, [f32; 3])] = &[
        (HumanBone::Hips, [0.0, 0.916_7, -0.050_1]),
        (HumanBone::UpperChest, [0.0, 1.314_8, -0.004_9]),
        (HumanBone::Head, [0.0, 1.568_7, 0.005_1]),
        (HumanBone::LeftHand, [0.738_9, 1.440_8, -0.065_4]),
        (HumanBone::RightHand, [-0.738_9, 1.440_8, -0.065_4]),
        (HumanBone::LeftFoot, [0.089, 0.103_7, -0.035_8]),
        (HumanBone::LeftToes, [0.089, 0.015_2, 0.113_2]),
    ];

    #[test]
    fn embedded_rest_reproduces_the_pack_skeleton() {
        let arm = armature();
        let heads = arm.world_heads();
        for &(bone, expected) in GOLDEN {
            let i = arm.name_to_index[bone.as_str()];
            let got = heads[i];
            let want = Vec3::from_array(expected);
            assert!(
                (got - want).length() < 1e-4,
                "{bone:?}: got {got:?}, want {want:?}"
            );
        }
    }

    #[test]
    fn armature_matches_the_fitted_glb_shape() {
        let arm = armature();
        // `Rig` + `root` + 52 joints, and a skin of root + 52 — the same
        // shape a pack-derived fit writes today.
        assert_eq!(arm.joints.len(), JOINT_COUNT + 2);
        assert_eq!(arm.skin.len(), JOINT_COUNT + 1);
        assert_eq!(arm.joints[0].name, ARMATURE_NAME);
        assert_eq!(arm.joints[ROOT_NODE].name, ROOT_NAME);
        assert_eq!(arm.skin[0], ROOT_NODE);
        assert_eq!(arm.joints[arm.skin[1]].name, HumanBone::Hips.as_str());
        // Hips hangs off `root`, not off the armature wrapper.
        assert_eq!(arm.joints[arm.skin[1]].parent, Some(ROOT_NODE));
        for bone in HumanBone::ALL {
            assert!(arm.name_to_index.contains_key(bone.as_str()), "{bone:?}");
        }
    }

    #[test]
    fn canonical_armature_is_upright_and_human_scaled() {
        let arm = armature();
        let heads = arm.world_heads();
        let head = heads[arm.name_to_index[HumanBone::Head.as_str()]];
        let foot = heads[arm.name_to_index[HumanBone::LeftFoot.as_str()]];
        assert!(
            (1.4..1.8).contains(&head.y),
            "roughly adult height, got {}",
            head.y
        );
        assert!(head.y > foot.y, "the root Z-up quarter turn was applied");
    }

    #[test]
    fn rest_is_indexed_by_discriminant() {
        // `HumanBone::rest` indexes REST directly; keep the orders locked.
        for (i, bone) in HumanBone::ALL.into_iter().enumerate() {
            assert_eq!(bone as usize, i, "{bone:?} discriminant");
            assert_eq!(REST[i].bone, bone, "REST[{i}]");
            assert_eq!(bone.rest().bone, bone);
        }
    }

    #[test]
    fn rest_is_parent_first() {
        let mut seen: HashSet<HumanBone> = HashSet::new();
        for joint in &REST {
            if let Some(p) = joint.parent {
                assert!(seen.contains(&p), "{:?} precedes parent {p:?}", joint.bone);
            }
            seen.insert(joint.bone);
        }
        assert_eq!(REST[0].bone, HumanBone::Hips, "hips is the humanoid root");
        assert!(REST[0].parent.is_none());
    }

    #[test]
    fn every_scheme_names_all_joints_uniquely() {
        for scheme in BoneScheme::ALL {
            let names: HashSet<&str> = HumanBone::ALL.iter().map(|b| b.name_in(scheme)).collect();
            assert_eq!(names.len(), JOINT_COUNT, "{scheme:?} has duplicate names");
            for bone in HumanBone::ALL {
                assert_eq!(scheme.to_canonical(bone.name_in(scheme)), Some(bone));
            }
        }
    }

    #[test]
    fn parse_round_trips_canonical_names() {
        for bone in HumanBone::ALL {
            assert_eq!(HumanBone::parse(bone.as_str()), Some(bone));
        }
        assert_eq!(HumanBone::parse("DEF-hips"), None);
        assert_eq!(HumanBone::parse(""), None);
    }

    #[test]
    fn pack_naming_quirks_are_mapped() {
        // VRM calls the fourth finger "little"; both packs call it "pinky".
        assert_eq!(HumanBone::LeftLittleProximal.unreal_name(), "pinky_01_l");
        // VRM's thumb starts at the metacarpal, not the proximal.
        assert_eq!(HumanBone::LeftThumbMetacarpal.unreal_name(), "thumb_01_l");
        // Semantic spine names against indexed ones.
        assert_eq!(HumanBone::UpperChest.unreal_name(), "spine_03");
    }

    #[test]
    fn detect_picks_the_right_scheme() {
        let unreal: Vec<&str> = HumanBone::ALL.iter().map(|b| b.unreal_name()).collect();
        let vrm: Vec<&str> = HumanBone::ALL.iter().map(|b| b.as_str()).collect();
        assert_eq!(BoneScheme::detect(unreal), Some(BoneScheme::Unreal));
        assert_eq!(BoneScheme::detect(vrm), Some(BoneScheme::Vrm));
    }

    #[test]
    fn detect_ignores_non_humanoid_nodes() {
        // An ordinary prop must not read as a rig just because of one node.
        assert_eq!(BoneScheme::detect(["mesh", "head", "Cube", "root"]), None);
        assert_eq!(BoneScheme::detect(Vec::<&str>::new()), None);
    }

    #[test]
    fn groups_cover_the_placeable_set() {
        let placeable = HumanBone::ALL.iter().filter(|b| b.is_placeable()).count();
        assert_eq!(placeable, 22, "22 major deformers, 30 finger joints");
        assert!(HumanBone::LeftHand.is_placeable());
        assert!(!HumanBone::LeftIndexProximal.is_placeable());
        assert!(HumanBone::LeftIndexProximal.is_finger());
        // Adjacent markers must not share a color.
        assert_ne!(HumanBone::Hips.group(), HumanBone::LeftUpperLeg.group());
        assert_ne!(
            HumanBone::LeftShoulder.group(),
            HumanBone::LeftUpperArm.group()
        );
    }

    #[test]
    fn sides_are_labeled_for_paired_joints() {
        assert_eq!(HumanBone::LeftUpperArm.side(), Some(Side::Left));
        assert_eq!(HumanBone::RightUpperArm.side(), Some(Side::Right));
        assert_eq!(HumanBone::Hips.side(), None);
        assert_eq!(HumanBone::UpperChest.side(), None);
        let paired = HumanBone::ALL.iter().filter(|b| b.side().is_some()).count();
        assert_eq!(paired, 46, "6 center joints, 23 per side");
    }

    #[test]
    fn panel_labels_match_the_legend() {
        assert_eq!(HumanBone::LeftShoulder.panel_label(), "Left clavicle");
        assert_eq!(HumanBone::RightShoulder.panel_label(), "Right clavicle");
        assert_eq!(HumanBone::LeftUpperArm.panel_label(), "Left shoulder");
        assert_eq!(HumanBone::RightUpperArm.panel_label(), "Right shoulder");
        assert_eq!(HumanBone::LeftLowerArm.panel_label(), "Left elbow");
        assert_eq!(HumanBone::LeftHand.panel_label(), "Left wrist");
        assert_eq!(HumanBone::LeftUpperLeg.panel_label(), "Left hip");
        assert_eq!(HumanBone::Hips.panel_label(), "Pelvis");
        assert_eq!(HumanBone::Head.panel_label(), "Head");
    }
}
